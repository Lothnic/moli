use std::io::Read;

use anyhow::{Context, Result, anyhow, bail, ensure};
use flate2::read::DeflateDecoder;

const WORDPROCESSING_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const DOCX_MAIN_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const MAX_CONTENT_TYPES_BYTES: usize = 1024 * 1024;
const MAX_DOCUMENT_XML_BYTES: usize = 32 * 1024 * 1024;

pub(super) fn render_document_markdown(body: &[u8]) -> Result<Vec<u8>> {
    let text = if body.starts_with(b"%PDF-") {
        render_pdf_text(body)?
    } else if body.starts_with(b"PK\x03\x04") {
        render_docx_text(body)?
    } else {
        bail!(
            "raw download cannot be converted to Markdown; native Markdown supports PDF and DOCX documents"
        );
    };
    let text = text.trim();
    ensure!(
        !text.is_empty(),
        "raw document contains no extractable text"
    );
    Ok(text.as_bytes().to_vec())
}

fn render_pdf_text(body: &[u8]) -> Result<String> {
    let document = lopdf::Document::load_mem(body).context("failed to parse raw PDF document")?;
    let pages = document.get_pages().keys().copied().collect::<Vec<_>>();
    let text = document
        .extract_text(&pages)
        .context("failed to extract text from raw PDF document")?
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{000c}', "\n\n");
    ensure!(
        !text.trim().is_empty(),
        "raw PDF document contains no extractable text"
    );
    Ok(text)
}

fn render_docx_text(body: &[u8]) -> Result<String> {
    let content_types = zip_entry(body, "[Content_Types].xml", MAX_CONTENT_TYPES_BYTES)
        .context("invalid DOCX package")?;
    let content_types =
        std::str::from_utf8(&content_types).context("DOCX content-types manifest is not UTF-8")?;
    let manifest = roxmltree::Document::parse(content_types)
        .context("failed to parse DOCX content-types manifest")?;
    ensure!(
        manifest.descendants().any(|node| {
            node.is_element()
                && node.tag_name().name() == "Override"
                && node.attribute("PartName") == Some("/word/document.xml")
                && node.attribute("ContentType") == Some(DOCX_MAIN_CONTENT_TYPE)
        }),
        "ZIP package is not a DOCX document"
    );

    let document = zip_entry(body, "word/document.xml", MAX_DOCUMENT_XML_BYTES)
        .context("invalid DOCX package")?;
    let document = std::str::from_utf8(&document).context("DOCX main document is not UTF-8")?;
    let xml = roxmltree::Document::parse(document).context("failed to parse DOCX main document")?;
    let body = xml
        .descendants()
        .find(|node| is_word_node(*node, "body"))
        .ok_or_else(|| anyhow!("DOCX main document has no body"))?;
    let mut blocks = Vec::new();
    render_blocks(body, &mut blocks);
    let markdown = blocks
        .into_iter()
        .filter(|block| !block.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    ensure!(
        !markdown.trim().is_empty(),
        "raw DOCX document contains no extractable text"
    );
    Ok(markdown)
}

fn render_blocks(parent: roxmltree::Node<'_, '_>, blocks: &mut Vec<String>) {
    for node in parent.children().filter(roxmltree::Node::is_element) {
        if is_word_node(node, "p") {
            if let Some(paragraph) = render_paragraph(node) {
                blocks.push(paragraph);
            }
        } else if is_word_node(node, "tbl") {
            if let Some(table) = render_table(node) {
                blocks.push(table);
            }
        } else {
            render_blocks(node, blocks);
        }
    }
}

fn render_paragraph(paragraph: roxmltree::Node<'_, '_>) -> Option<String> {
    let mut text = String::new();
    for node in paragraph.descendants().filter(roxmltree::Node::is_element) {
        if is_word_node(node, "t") {
            text.push_str(node.text().unwrap_or_default());
        } else if is_word_node(node, "tab") {
            text.push('\t');
        } else if is_word_node(node, "br") || is_word_node(node, "cr") {
            text.push('\n');
        }
    }
    let text = text.lines().map(str::trim).collect::<Vec<_>>().join("\n");
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let style = paragraph
        .descendants()
        .find(|node| is_word_node(*node, "pStyle"))
        .and_then(|node| word_attribute(node, "val"));
    if let Some(level) = style.as_deref().and_then(heading_level) {
        return Some(format!("{} {text}", "#".repeat(level)));
    }
    let is_list = paragraph
        .descendants()
        .any(|node| is_word_node(node, "numPr"));
    Some(if is_list {
        format!("- {text}")
    } else {
        text.to_owned()
    })
}

fn render_table(table: roxmltree::Node<'_, '_>) -> Option<String> {
    let mut rows = table
        .children()
        .filter(|node| is_word_node(*node, "tr"))
        .filter_map(|row| {
            let cells = row
                .children()
                .filter(|node| is_word_node(*node, "tc"))
                .map(|cell| {
                    cell.descendants()
                        .filter(|node| {
                            is_word_node(*node, "p")
                                && node
                                    .ancestors()
                                    .find(|ancestor| is_word_node(*ancestor, "tc"))
                                    == Some(cell)
                        })
                        .filter_map(render_paragraph)
                        .collect::<Vec<_>>()
                        .join("<br>")
                        .replace('\\', "\\\\")
                        .replace('|', "\\|")
                })
                .collect::<Vec<_>>();
            (!cells.iter().all(String::is_empty)).then(|| format!("| {} |", cells.join(" | ")))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return None;
    }
    let column_count = rows[0].matches('|').count().saturating_sub(1);
    if column_count > 0 {
        rows.insert(1, format!("| {} |", vec!["---"; column_count].join(" | ")));
    }
    Some(rows.join("\n"))
}

fn is_word_node(node: roxmltree::Node<'_, '_>, local_name: &str) -> bool {
    node.is_element()
        && node.tag_name().namespace() == Some(WORDPROCESSING_NAMESPACE)
        && node.tag_name().name() == local_name
}

fn word_attribute(node: roxmltree::Node<'_, '_>, local_name: &str) -> Option<String> {
    node.attributes()
        .find(|attribute| {
            attribute.namespace() == Some(WORDPROCESSING_NAMESPACE)
                && attribute.name() == local_name
        })
        .map(|attribute| attribute.value().to_owned())
}

fn heading_level(style: &str) -> Option<usize> {
    let suffix = style
        .strip_prefix("Heading")
        .or_else(|| style.strip_prefix("heading"))?;
    suffix
        .parse::<usize>()
        .ok()
        .filter(|level| (1..=6).contains(level))
}

fn zip_entry(archive: &[u8], wanted_name: &str, max_bytes: usize) -> Result<Vec<u8>> {
    let eocd = archive
        .len()
        .checked_sub(22)
        .and_then(|last| {
            let first = archive.len().saturating_sub(22 + u16::MAX as usize);
            (first..=last)
                .rev()
                .find(|offset| archive.get(*offset..*offset + 4) == Some(b"PK\x05\x06"))
        })
        .ok_or_else(|| anyhow!("ZIP end-of-central-directory record is missing"))?;
    ensure!(
        read_u16(archive, eocd + 4)? == 0,
        "multi-disk ZIP is unsupported"
    );
    ensure!(
        read_u16(archive, eocd + 6)? == 0,
        "multi-disk ZIP is unsupported"
    );
    let entries = usize::from(read_u16(archive, eocd + 10)?);
    let central_size = read_u32(archive, eocd + 12)? as usize;
    let central_offset = read_u32(archive, eocd + 16)? as usize;
    let central_end = central_offset
        .checked_add(central_size)
        .ok_or_else(|| anyhow!("ZIP central directory offset overflow"))?;
    ensure!(central_end <= eocd, "ZIP central directory is truncated");

    let mut cursor = central_offset;
    for _ in 0..entries {
        ensure!(
            archive.get(cursor..cursor + 4) == Some(b"PK\x01\x02"),
            "invalid ZIP central-directory entry"
        );
        let flags = read_u16(archive, cursor + 8)?;
        let compression = read_u16(archive, cursor + 10)?;
        let expected_crc = read_u32(archive, cursor + 16)?;
        let compressed_size = read_u32(archive, cursor + 20)? as usize;
        let uncompressed_size = read_u32(archive, cursor + 24)? as usize;
        let name_len = usize::from(read_u16(archive, cursor + 28)?);
        let extra_len = usize::from(read_u16(archive, cursor + 30)?);
        let comment_len = usize::from(read_u16(archive, cursor + 32)?);
        let local_offset = read_u32(archive, cursor + 42)? as usize;
        let name_start = cursor + 46;
        let name_end = checked_add(name_start, name_len)?;
        let name = std::str::from_utf8(
            archive
                .get(name_start..name_end)
                .ok_or_else(|| anyhow!("truncated ZIP entry name"))?,
        )
        .context("ZIP entry name is not UTF-8")?;
        cursor = checked_add(name_end, checked_add(extra_len, comment_len)?)?;
        ensure!(cursor <= central_end, "truncated ZIP central directory");
        if name != wanted_name {
            continue;
        }
        ensure!(flags & 0x0001 == 0, "encrypted ZIP entries are unsupported");
        ensure!(
            uncompressed_size <= max_bytes,
            "DOCX part exceeds {max_bytes} bytes"
        );
        ensure!(
            archive.get(local_offset..local_offset + 4) == Some(b"PK\x03\x04"),
            "invalid ZIP local-file header"
        );
        let local_name_len = usize::from(read_u16(archive, local_offset + 26)?);
        let local_extra_len = usize::from(read_u16(archive, local_offset + 28)?);
        let data_start = checked_add(
            local_offset + 30,
            checked_add(local_name_len, local_extra_len)?,
        )?;
        let data_end = checked_add(data_start, compressed_size)?;
        let compressed = archive
            .get(data_start..data_end)
            .ok_or_else(|| anyhow!("truncated ZIP entry data"))?;
        let bytes = match compression {
            0 => compressed.to_vec(),
            8 => {
                let mut decoder = DeflateDecoder::new(compressed);
                let mut bytes = Vec::with_capacity(uncompressed_size.min(max_bytes));
                decoder
                    .by_ref()
                    .take(max_bytes.saturating_add(1) as u64)
                    .read_to_end(&mut bytes)
                    .context("failed to inflate DOCX part")?;
                bytes
            }
            method => bail!("unsupported DOCX ZIP compression method {method}"),
        };
        ensure!(
            bytes.len() == uncompressed_size && bytes.len() <= max_bytes,
            "DOCX part size does not match ZIP metadata"
        );
        ensure!(crc32(&bytes) == expected_crc, "DOCX part CRC mismatch");
        return Ok(bytes);
    }
    bail!("DOCX package is missing {wanted_name}")
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| anyhow!("truncated ZIP structure"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| anyhow!("truncated ZIP structure"))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn checked_add(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right)
        .ok_or_else(|| anyhow!("ZIP entry offset overflow"))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}
