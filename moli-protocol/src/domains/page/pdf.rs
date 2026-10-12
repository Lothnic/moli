use crate::automation::DevToolsPrintToPdfCommand;

const POINTS_PER_INCH: f64 = 72.0;
const DEFAULT_MARGIN_INCHES: f64 = 1.0 / 2.54;
const DEFAULT_PAGE_WIDTH_INCHES: f64 = 8.5;
const DEFAULT_PAGE_HEIGHT_INCHES: f64 = 11.0;
const MAX_PAGE_DIMENSION_INCHES: f64 = 200.0;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PrintPdfOptions {
    page_width_points: f64,
    page_height_points: f64,
    margin_top_points: f64,
    margin_bottom_points: f64,
    margin_left_points: f64,
    margin_right_points: f64,
    scale: f64,
    page_ranges: Vec<PageRange>,
}

impl Default for PrintPdfOptions {
    fn default() -> Self {
        Self {
            page_width_points: DEFAULT_PAGE_WIDTH_INCHES * POINTS_PER_INCH,
            page_height_points: DEFAULT_PAGE_HEIGHT_INCHES * POINTS_PER_INCH,
            margin_top_points: DEFAULT_MARGIN_INCHES * POINTS_PER_INCH,
            margin_bottom_points: DEFAULT_MARGIN_INCHES * POINTS_PER_INCH,
            margin_left_points: DEFAULT_MARGIN_INCHES * POINTS_PER_INCH,
            margin_right_points: DEFAULT_MARGIN_INCHES * POINTS_PER_INCH,
            scale: 1.0,
            page_ranges: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PageRange {
    first: usize,
    last: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PdfError {
    code: i32,
    message: String,
}

impl PdfError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }

    fn server(message: impl Into<String>) -> Self {
        Self {
            code: -32000,
            message: message.into(),
        }
    }

    pub(super) fn code(&self) -> i32 {
        self.code
    }

    pub(super) fn message(&self) -> &str {
        &self.message
    }
}

impl PrintPdfOptions {
    pub(super) fn from_command(command: &DevToolsPrintToPdfCommand) -> Result<Self, PdfError> {
        let scale = command.scale.unwrap_or(1.0);
        if !scale.is_finite() || !(0.1..=2.0).contains(&scale) {
            return Err(PdfError::invalid_params(
                "scale is outside of [0.1 - 2] range",
            ));
        }

        let margin_left = finite_margin(command.margin_left, "left margin is negative")?;
        let margin_right = finite_margin(command.margin_right, "right margin is negative")?;
        let margin_top = finite_margin(command.margin_top, "top margin is negative")?;
        let margin_bottom = finite_margin(command.margin_bottom, "bottom margin is negative")?;
        let mut paper_width = finite_paper_dimension(
            command.paper_width,
            DEFAULT_PAGE_WIDTH_INCHES,
            "paper width is zero or negative",
        )?;
        let mut paper_height = finite_paper_dimension(
            command.paper_height,
            DEFAULT_PAGE_HEIGHT_INCHES,
            "paper height is zero or negative",
        )?;
        if command.landscape.unwrap_or(false) {
            std::mem::swap(&mut paper_width, &mut paper_height);
        }
        if paper_width > MAX_PAGE_DIMENSION_INCHES || paper_height > MAX_PAGE_DIMENSION_INCHES {
            return Err(PdfError::invalid_params("invalid print parameters"));
        }
        if paper_width <= margin_left + margin_right || paper_height <= margin_top + margin_bottom {
            return Err(PdfError::invalid_params(
                "invalid print parameters: printable area is empty",
            ));
        }

        Ok(Self {
            page_width_points: paper_width * POINTS_PER_INCH,
            page_height_points: paper_height * POINTS_PER_INCH,
            margin_top_points: margin_top * POINTS_PER_INCH,
            margin_bottom_points: margin_bottom * POINTS_PER_INCH,
            margin_left_points: margin_left * POINTS_PER_INCH,
            margin_right_points: margin_right * POINTS_PER_INCH,
            scale,
            page_ranges: parse_page_ranges(command.page_ranges.as_deref().unwrap_or(""))?,
        })
    }
}

fn finite_margin(value: Option<f64>, negative_message: &'static str) -> Result<f64, PdfError> {
    let value = value.unwrap_or(DEFAULT_MARGIN_INCHES);
    if !value.is_finite() {
        return Err(PdfError::invalid_params("invalid print parameters"));
    }
    if value < 0.0 {
        return Err(PdfError::invalid_params(negative_message));
    }
    Ok(value)
}

fn finite_paper_dimension(
    value: Option<f64>,
    default: f64,
    non_positive_message: &'static str,
) -> Result<f64, PdfError> {
    let value = value.unwrap_or(default);
    if !value.is_finite() {
        return Err(PdfError::invalid_params("invalid print parameters"));
    }
    if value <= 0.0 {
        return Err(PdfError::invalid_params(non_positive_message));
    }
    Ok(value)
}

fn parse_page_ranges(value: &str) -> Result<Vec<PageRange>, PdfError> {
    let mut ranges = Vec::new();
    for range in value
        .split(',')
        .map(str::trim)
        .filter(|range| !range.is_empty())
    {
        let (first, last) = if range == "-" {
            (1, usize::MAX)
        } else if let Some(last) = range.strip_prefix('-') {
            (1, parse_page_number(last)?)
        } else if let Some(first) = range.strip_suffix('-') {
            (parse_page_number(first)?, usize::MAX)
        } else if range.contains('-') {
            let mut parts = range.split('-').map(str::trim);
            let first = parts.next().ok_or_else(page_range_syntax_error)?;
            let last = parts.next().ok_or_else(page_range_syntax_error)?;
            if parts.next().is_some() {
                return Err(page_range_syntax_error());
            }
            (parse_page_number(first)?, parse_page_number(last)?)
        } else {
            let page = parse_page_number(range)?;
            (page, page)
        };
        if first == 0 || first > last {
            return Err(PdfError::server("Page range is invalid (start > end)"));
        }
        ranges.push(PageRange {
            first: first - 1,
            last: last.saturating_sub(1),
        });
    }
    Ok(ranges)
}

fn parse_page_number(value: &str) -> Result<usize, PdfError> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(page_range_syntax_error());
    }
    value.parse().map_err(|_| page_range_syntax_error())
}

fn page_range_syntax_error() -> PdfError {
    PdfError::server("Page range syntax error")
}

impl PrintPdfOptions {
    pub(super) fn into_renderer_options(self) -> moli_core::page::RendererPdfOptions {
        moli_core::page::RendererPdfOptions {
            width: self.page_width_points,
            height: self.page_height_points,
            margin_top: self.margin_top_points,
            margin_bottom: self.margin_bottom_points,
            margin_left: self.margin_left_points,
            margin_right: self.margin_right_points,
            scale: self.scale,
            page_ranges: self
                .page_ranges
                .into_iter()
                .map(|range| range.first..=range.last)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::{AutomationContext, DevToolsPrintToPdfTransferMode, FrontendProtocol};

    fn build_pdf(
        snapshot: &moli_layout::PaintSnapshot,
        options: &PrintPdfOptions,
    ) -> Result<Vec<u8>, PdfError> {
        moli_paint::pdf::pdf_snapshot(snapshot, &options.clone().into_renderer_options())
            .map_err(|error| PdfError::server(error.to_string()))
    }

    fn snapshot(width: u32, height: u32) -> moli_layout::PaintSnapshot {
        moli_layout::PaintSnapshot::new(
            moli_layout::PaintViewport::new(width, height, 1.0),
            moli_layout::PaintColor::WHITE,
        )
    }

    fn command() -> DevToolsPrintToPdfCommand {
        DevToolsPrintToPdfCommand {
            context: AutomationContext {
                protocol: FrontendProtocol::Cdp,
                session_id: None,
                target_id: None,
                browser_context_id: None,
            },
            landscape: None,
            print_background: None,
            scale: None,
            paper_width: None,
            paper_height: None,
            margin_top: None,
            margin_bottom: None,
            margin_left: None,
            margin_right: None,
            page_ranges: None,
            shrink_to_fit: None,
            transfer_mode: Some(DevToolsPrintToPdfTransferMode::ReturnAsBase64),
        }
    }

    #[test]
    fn writes_cross_referenced_paginated_pdf() {
        let mut command = command();
        command.paper_width = Some(4.0);
        command.paper_height = Some(4.0);
        command.margin_top = Some(0.0);
        command.margin_bottom = Some(0.0);
        command.margin_left = Some(0.0);
        command.margin_right = Some(0.0);
        let options = PrintPdfOptions::from_command(&command).unwrap();
        let pdf = build_pdf(&snapshot(400, 900), &options).unwrap();
        let text = String::from_utf8_lossy(&pdf);

        assert!(pdf.starts_with(b"%PDF-1.7"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        assert!(text.contains("/Count 3"));
        assert_eq!(
            lopdf::Document::load_mem(&pdf).unwrap().get_pages().len(),
            3
        );
        assert_eq!(text.matches("/Subtype /Image").count(), 0);

        let xref_offset = text
            .rsplit_once("startxref\n")
            .and_then(|(_, tail)| tail.lines().next())
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(&pdf[xref_offset..xref_offset + 4], b"xref");
    }

    #[test]
    fn landscape_swaps_media_box_and_ranges_are_normalized() {
        let mut command = command();
        command.landscape = Some(true);
        command.paper_width = Some(8.0);
        command.paper_height = Some(10.0);
        command.margin_top = Some(0.0);
        command.margin_bottom = Some(0.0);
        command.margin_left = Some(0.0);
        command.margin_right = Some(0.0);
        command.page_ranges = Some("3, 1-2,2".to_owned());
        let options = PrintPdfOptions::from_command(&command).unwrap();
        let pdf = build_pdf(&snapshot(800, 2400), &options).unwrap();
        let document = lopdf::Document::load_mem(&pdf).unwrap();
        assert_eq!(document.get_pages().len(), 3);
        for id in document.get_pages().values() {
            let media_box = document
                .get_dictionary(*id)
                .unwrap()
                .get(b"MediaBox")
                .unwrap()
                .as_array()
                .unwrap();
            let dimensions: Vec<_> = media_box
                .iter()
                .map(|value| value.as_float().unwrap())
                .collect();
            assert_eq!(dimensions, vec![0.0, 0.0, 720.0, 576.0]);
        }
    }

    #[test]
    fn validates_chromium_scale_and_page_range_errors() {
        let mut invalid_scale = command();
        invalid_scale.scale = Some(2.01);
        let error = PrintPdfOptions::from_command(&invalid_scale).unwrap_err();
        assert_eq!(error.code(), -32602);
        assert_eq!(error.message(), "scale is outside of [0.1 - 2] range");

        let mut invalid_range = command();
        invalid_range.page_ranges = Some("4-2".to_owned());
        let error = PrintPdfOptions::from_command(&invalid_range).unwrap_err();
        assert_eq!(error.code(), -32000);
        assert_eq!(error.message(), "Page range is invalid (start > end)");

        let mut syntax_error = command();
        syntax_error.page_ranges = Some("1-a".to_owned());
        let error = PrintPdfOptions::from_command(&syntax_error).unwrap_err();
        assert_eq!(error.message(), "Page range syntax error");
    }

    #[test]
    fn rejects_ranges_wholly_beyond_document() {
        let mut command = command();
        command.page_ranges = Some("998-999".to_owned());
        command.margin_top = Some(0.0);
        command.margin_bottom = Some(0.0);
        command.margin_left = Some(0.0);
        command.margin_right = Some(0.0);
        let options = PrintPdfOptions::from_command(&command).unwrap();
        let error = build_pdf(&snapshot(800, 600), &options).unwrap_err();
        assert_eq!(error.message(), "Page range exceeds page count");
    }
}
