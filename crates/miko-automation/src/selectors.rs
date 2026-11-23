//! Matching primitives shared by every workflow entry point.
use miko_core::{MikoError, MikoResult};

use crate::Point;

/// Find one UI node by exact text and/or resource ID. Refuse ambiguous matches.
pub fn find_ui(
    xml: &str,
    text: Option<&str>,
    resource_id: Option<&str>,
    width: u32,
    height: u32,
) -> MikoResult<Option<Point>> {
    if text.is_none_or(str::is_empty) && resource_id.is_none_or(str::is_empty) {
        return Err(MikoError::usage("UI selector is empty"));
    }
    let document = roxmltree::Document::parse(xml)
        .map_err(|e| MikoError::unexpected(format!("invalid UI hierarchy: {e}")))?;
    let mut found = None;
    for node in document.descendants().filter(|n| n.has_tag_name("node")) {
        if text.is_some_and(|value| node.attribute("text") != Some(value))
            || resource_id.is_some_and(|value| node.attribute("resource-id") != Some(value))
        {
            continue;
        }
        let bounds = node
            .attribute("bounds")
            .ok_or_else(|| MikoError::unexpected("matching UI element has no bounds"))?;
        let coordinates = bounds
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .and_then(|s| s.split_once("]["))
            .and_then(|(a, b)| Some((a.split_once(',')?, b.split_once(',')?)))
            .ok_or_else(|| MikoError::unexpected("invalid UI element bounds"))?;
        let values: Vec<u32> = [
            coordinates.0.0,
            coordinates.0.1,
            coordinates.1.0,
            coordinates.1.1,
        ]
        .into_iter()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| MikoError::unexpected("invalid UI element bounds"))?;
        if values.len() != 4
            || width == 0
            || height == 0
            || values[0] >= values[2]
            || values[1] >= values[3]
            || values[2] > width
            || values[3] > height
        {
            return Err(MikoError::unexpected("invalid UI element bounds"));
        }
        let point = Point {
            x: (f64::from(values[0]) + f64::from(values[2])) / (2.0 * f64::from(width)),
            y: (f64::from(values[1]) + f64::from(values[3])) / (2.0 * f64::from(height)),
        };
        if found.replace(point).is_some() {
            return Err(MikoError::usage("multiple UI elements match"));
        }
    }
    Ok(found)
}

/// Decoded RGBA image. A template's alpha channel masks transparent pixels.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    fn valid(&self) -> bool {
        u64::from(self.width) * u64::from(self.height) * 4 == self.pixels.len() as u64
            && self.width > 0
            && self.height > 0
    }
}

/// Match at the screenshot's native size using normalized mean RGB error.
/// Nearby overlapping hits are one candidate; distinct hits are ambiguous.
pub fn find_template(
    screen: &Image,
    template: &Image,
    threshold: f64,
) -> MikoResult<Option<Point>> {
    find_template_cancellable(screen, template, threshold, || false)
}

/// As above, stopping promptly when the caller cancels a background search.
pub fn find_template_cancellable(
    screen: &Image,
    template: &Image,
    threshold: f64,
    cancelled: impl Fn() -> bool,
) -> MikoResult<Option<Point>> {
    if !screen.valid()
        || !template.valid()
        || !threshold.is_finite()
        || !(0.0..=1.0).contains(&threshold)
    {
        return Err(MikoError::usage("invalid template image or threshold"));
    }
    if template.width > screen.width || template.height > screen.height {
        return Ok(None);
    }
    let count = template
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] != 0)
        .count();
    if count == 0 {
        return Err(MikoError::usage("template has no visible pixels"));
    }
    let max_error = (1.0 - threshold) * count as f64 * 3.0 * 255.0;
    let mut best: Option<(u32, u32, u64)> = None;
    for y in 0..=screen.height - template.height {
        if cancelled() {
            return Err(miko_core::cancel::cancelled());
        }
        for x in 0..=screen.width - template.width {
            let mut error = 0u64;
            'pixels: for ty in 0..template.height {
                for tx in 0..template.width {
                    let ti = ((ty * template.width + tx) * 4) as usize;
                    if template.pixels[ti + 3] == 0 {
                        continue;
                    }
                    let si = (((y + ty) * screen.width + x + tx) * 4) as usize;
                    for c in 0..3 {
                        error += u64::from(template.pixels[ti + c].abs_diff(screen.pixels[si + c]));
                    }
                    if error as f64 > max_error {
                        break 'pixels;
                    }
                }
            }
            if error as f64 > max_error {
                continue;
            }
            if let Some((bx, by, old_error)) = best {
                let overlaps = x.abs_diff(bx) < template.width && y.abs_diff(by) < template.height;
                if !overlaps {
                    return Err(MikoError::usage("multiple template matches"));
                }
                if error < old_error {
                    best = Some((x, y, error));
                }
            } else {
                best = Some((x, y, error));
            }
        }
    }
    Ok(best.map(|(x, y, _)| Point {
        x: (f64::from(x) + f64::from(template.width) / 2.0) / f64::from(screen.width),
        y: (f64::from(y) + f64::from(template.height) / 2.0) / f64::from(screen.height),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_matching_decodes_xml_and_rejects_ambiguity() {
        let xml = r#"<hierarchy><node text="A &amp; B" resource-id="app:id/a" bounds="[10,20][30,40]"/></hierarchy>"#;
        assert_eq!(
            find_ui(xml, Some("A & B"), Some("app:id/a"), 100, 100).unwrap(),
            Some(Point { x: 0.2, y: 0.3 })
        );
        let duplicate = xml.replace(
            "</hierarchy>",
            "<node text=\"A &amp; B\" bounds=\"[40,40][50,50]\"/></hierarchy>",
        );
        assert!(find_ui(&duplicate, Some("A & B"), None, 100, 100).is_err());
    }

    #[test]
    fn template_matches_and_rejects_distinct_hits() {
        let screen = Image {
            width: 3,
            height: 1,
            pixels: vec![0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255],
        };
        let white = Image {
            width: 1,
            height: 1,
            pixels: vec![255, 255, 255, 255],
        };
        let black = Image {
            width: 1,
            height: 1,
            pixels: vec![0, 0, 0, 255],
        };
        assert_eq!(
            find_template(&screen, &white, 1.0).unwrap(),
            Some(Point { x: 0.5, y: 0.5 })
        );
        assert!(find_template(&screen, &black, 1.0).is_err());
        assert!(miko_core::cancel::is_cancelled(
            &find_template_cancellable(&screen, &white, 1.0, || true).unwrap_err()
        ));
    }
}
