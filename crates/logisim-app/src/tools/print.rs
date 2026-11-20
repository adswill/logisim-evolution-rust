//! Printing. Rust has no cross-platform print dialog, so we build a print-ready HTML page (one
//! circuit per page, scaled to fit) and open it in the system browser, which shows the normal
//! print dialog (and offers "Save as PDF").
//!
//! Entry points: [`print_html`] (pure, testable) and [`open_for_printing`] (writes a temp file,
//! opens the browser).

use super::export::{export_svg, ExportOptions};
use logisim_format::ProjectFile;
use std::path::PathBuf;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// HTML page with one section per circuit. The page calls `window.print()` when it loads.
pub fn print_html(
    project: &ProjectFile,
    circuits: &[String],
    landscape: bool,
) -> Result<String, String> {
    if circuits.is_empty() {
        return Err("Pick at least one circuit to print.".into());
    }
    let opts = ExportOptions {
        background: false,
        dark: false,
        ..Default::default()
    };
    let mut body = String::new();
    for name in circuits {
        let svg = export_svg(project, name, &opts)?;
        body += &format!(
            "<section><h2>{}</h2>\n{}</section>\n",
            esc(name),
            // drop the fixed width/height so CSS can scale the drawing to the page
            svg.replacen(" width=", " data-w=", 1)
                .replacen(" height=", " data-h=", 1)
        );
    }
    Ok(format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{title}</title>\n<style>\n\
         @page {{ size: {orient}; margin: 12mm; }}\n\
         body {{ font-family: sans-serif; margin: 0; }}\n\
         section {{ page-break-after: always; }}\n\
         section:last-child {{ page-break-after: auto; }}\n\
         h2 {{ font-size: 12pt; margin: 0 0 6mm; }}\n\
         svg {{ width: 100%; height: auto; max-height: 170mm; }}\n\
         </style></head><body>\n{body}<script>window.onload = function () {{ window.print(); }};</script>\n</body></html>\n",
        title = esc(&project.metadata.name),
        orient = if landscape { "A4 landscape" } else { "A4 portrait" },
    ))
}

/// Write the page to a temporary file and open it in the default browser.
pub fn open_for_printing(html: &str) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!("rcirc-print-{}.html", std::process::id()));
    std::fs::write(&path, html).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    let status = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&path).status()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(&path)
            .status()
    } else {
        std::process::Command::new("xdg-open").arg(&path).status()
    };
    match status {
        Ok(s) if s.success() => Ok(path),
        Ok(_) | Err(_) => Err(format!(
            "Could not open a browser automatically. Open {} yourself and print it.",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_has_one_section_per_circuit_and_prints() {
        let p = crate::examples::half_adder().project;
        let n = p.main.clone();
        let html = print_html(&p, &[n.clone(), n], true).unwrap();
        assert_eq!(html.matches("<section>").count(), 2);
        assert!(html.contains("window.print()"));
        assert!(html.contains("A4 landscape"));
        assert!(!html.contains(" width=\""), "svg must scale with the page");
    }

    #[test]
    fn nothing_selected_is_an_error() {
        let p = crate::examples::half_adder().project;
        assert!(print_html(&p, &[], false).is_err());
        assert!(print_html(&p, &["zzz".into()], false).is_err());
    }
}
