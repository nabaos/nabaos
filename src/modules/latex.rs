// LaTeX document generation module — templates for invoices, papers, reports, and letters.

use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::core::error::{NyayaError, Result};

/// Available LaTeX compilation backends.
#[derive(Debug, Clone)]
pub enum LatexBackend {
    Tectonic(PathBuf),
    PdfLatex(PathBuf),
    XeLatex(PathBuf),
    NotFound,
}

impl LatexBackend {
    /// Auto-detect the best available LaTeX backend on this system.
    pub fn detect() -> Self {
        if let Some(p) = super::hardware::detect_tool(&["tectonic"]) {
            return LatexBackend::Tectonic(p);
        }
        if let Some(p) = super::hardware::detect_tool(&["pdflatex"]) {
            return LatexBackend::PdfLatex(p);
        }
        if let Some(p) = super::hardware::detect_tool(&["xelatex"]) {
            return LatexBackend::XeLatex(p);
        }
        LatexBackend::NotFound
    }

    /// Compile a .tex file to PDF in the given output directory.
    pub fn compile(&self, tex_path: &Path, output_dir: &Path) -> Result<PathBuf> {
        let (bin, args): (&Path, Vec<String>) = match self {
            LatexBackend::Tectonic(p) => (
                p.as_path(),
                vec![
                    "--outdir".into(),
                    output_dir.to_string_lossy().into_owned(),
                    tex_path.to_string_lossy().into_owned(),
                ],
            ),
            LatexBackend::PdfLatex(p) | LatexBackend::XeLatex(p) => (
                p.as_path(),
                vec![
                    format!("-output-directory={}", output_dir.display()),
                    "-interaction=nonstopmode".into(),
                    tex_path.to_string_lossy().into_owned(),
                ],
            ),
            LatexBackend::NotFound => {
                return Err(NyayaError::Config(
                    "No LaTeX backend found. Install tectonic, pdflatex, or xelatex.".into(),
                ));
            }
        };

        let output = std::process::Command::new(bin)
            .args(&args)
            .output()
            .map_err(|e| NyayaError::Config(format!("Failed to run LaTeX compiler: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NyayaError::Config(format!(
                "LaTeX compilation failed: {}",
                stderr
            )));
        }

        let stem = tex_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        Ok(output_dir.join(format!("{}.pdf", stem)))
    }
}

/// A single line item on an invoice.
#[derive(Debug, Clone, Deserialize)]
pub struct InvoiceItem {
    pub description: String,
    pub quantity: f64,
    pub unit_price: f64,
}

/// Data for rendering an invoice template.
#[derive(Debug, Clone, Deserialize)]
pub struct InvoiceData {
    pub invoice_number: String,
    pub date: String,
    pub from_name: String,
    pub from_address: String,
    pub to_name: String,
    pub to_address: String,
    pub items: Vec<InvoiceItem>,
    pub tax_rate: f64,
    pub notes: Option<String>,
}

/// A section within a paper or report.
#[derive(Debug, Clone, Deserialize)]
pub struct PaperSection {
    pub title: String,
    pub content: String,
}

/// Data for rendering a research paper template.
#[derive(Debug, Clone, Deserialize)]
pub struct PaperData {
    pub title: String,
    pub authors: Vec<String>,
    pub abstract_text: String,
    pub sections: Vec<PaperSection>,
    pub references: Vec<String>,
}

/// Data for rendering a business report template.
#[derive(Debug, Clone, Deserialize)]
pub struct ReportData {
    pub title: String,
    pub author: String,
    pub date: String,
    pub executive_summary: String,
    pub sections: Vec<PaperSection>,
}

/// Data for rendering a formal letter template.
#[derive(Debug, Clone, Deserialize)]
pub struct LetterData {
    pub from_name: String,
    pub from_address: String,
    pub to_name: String,
    pub to_address: String,
    pub date: String,
    pub subject: String,
    pub body: String,
    pub closing: String,
}

/// Metadata about an available template.
#[derive(Debug, Clone)]
pub struct TemplateInfo {
    pub name: &'static str,
    pub description: &'static str,
}

/// Escape special LaTeX characters in user-provided text.
pub fn latex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\textbackslash{}"),
            '&' => out.push_str("\\&"),
            '%' => out.push_str("\\%"),
            '$' => out.push_str("\\$"),
            '#' => out.push_str("\\#"),
            '_' => out.push_str("\\_"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            other => out.push(other),
        }
    }
    out
}

/// Render a complete LaTeX invoice document.
pub fn render_invoice(data: &InvoiceData) -> String {
    let mut doc = String::new();

    doc.push_str("\\documentclass{article}\n");
    doc.push_str("\\usepackage[margin=1in]{geometry}\n");
    doc.push_str("\\usepackage{longtable}\n");
    doc.push_str("\\usepackage{booktabs}\n");
    doc.push_str("\\begin{document}\n\n");

    doc.push_str(&format!(
        "\\begin{{flushright}}\n\\textbf{{Invoice {}}} \\\\\n{}\n\\end{{flushright}}\n\n",
        latex_escape(&data.invoice_number),
        latex_escape(&data.date)
    ));

    doc.push_str(&format!(
        "\\textbf{{From:}} {} \\\\\n{}\n\n",
        latex_escape(&data.from_name),
        latex_escape(&data.from_address)
    ));
    doc.push_str(&format!(
        "\\textbf{{To:}} {} \\\\\n{}\n\n",
        latex_escape(&data.to_name),
        latex_escape(&data.to_address)
    ));

    doc.push_str("\\begin{longtable}{p{6cm} r r r}\n");
    doc.push_str("\\toprule\n");
    doc.push_str("Description & Qty & Unit Price & Total \\\\\n");
    doc.push_str("\\midrule\n");

    let mut subtotal = 0.0_f64;
    for item in &data.items {
        let total = item.quantity * item.unit_price;
        subtotal += total;
        doc.push_str(&format!(
            "{} & {:.0} & \\${:.2} & \\${:.2} \\\\\n",
            latex_escape(&item.description),
            item.quantity,
            item.unit_price,
            total
        ));
    }

    let tax = subtotal * data.tax_rate;
    let grand_total = subtotal + tax;

    doc.push_str("\\midrule\n");
    doc.push_str(&format!(
        "\\multicolumn{{3}}{{r}}{{Subtotal}} & \\${:.2} \\\\\n",
        subtotal
    ));
    doc.push_str(&format!(
        "\\multicolumn{{3}}{{r}}{{Tax ({:.0}\\%)}} & \\${:.2} \\\\\n",
        data.tax_rate * 100.0,
        tax
    ));
    doc.push_str(&format!(
        "\\multicolumn{{3}}{{r}}{{\\textbf{{Total}}}} & \\textbf{{\\${:.2}}} \\\\\n",
        grand_total
    ));
    doc.push_str("\\bottomrule\n");
    doc.push_str("\\end{longtable}\n\n");

    if let Some(ref notes) = data.notes {
        doc.push_str(&format!(
            "\\vspace{{1cm}}\n\\textbf{{Notes:}} {}\n\n",
            latex_escape(notes)
        ));
    }

    doc.push_str("\\end{document}\n");
    doc
}

/// Render a complete LaTeX research paper document.
pub fn render_paper(data: &PaperData) -> String {
    let mut doc = String::new();

    doc.push_str("\\documentclass{article}\n");
    doc.push_str("\\usepackage[margin=1in]{geometry}\n");
    doc.push_str("\\begin{document}\n\n");

    doc.push_str(&format!("\\title{{{}}}\n", latex_escape(&data.title)));
    doc.push_str(&format!(
        "\\author{{{}}}\n",
        data.authors
            .iter()
            .map(|a| latex_escape(a))
            .collect::<Vec<_>>()
            .join(" \\and ")
    ));
    doc.push_str("\\maketitle\n\n");

    doc.push_str("\\begin{abstract}\n");
    doc.push_str(&latex_escape(&data.abstract_text));
    doc.push_str("\n\\end{abstract}\n\n");

    for section in &data.sections {
        doc.push_str(&format!("\\section{{{}}}\n", latex_escape(&section.title)));
        doc.push_str(&latex_escape(&section.content));
        doc.push_str("\n\n");
    }

    if !data.references.is_empty() {
        doc.push_str(&format!(
            "\\begin{{thebibliography}}{{{}}}\n",
            data.references.len()
        ));
        for (i, reference) in data.references.iter().enumerate() {
            doc.push_str(&format!(
                "\\bibitem{{ref{}}} {}\n",
                i + 1,
                latex_escape(reference)
            ));
        }
        doc.push_str("\\end{thebibliography}\n\n");
    }

    doc.push_str("\\end{document}\n");
    doc
}

/// Render a complete LaTeX business report document.
pub fn render_report(data: &ReportData) -> String {
    let mut doc = String::new();

    doc.push_str("\\documentclass{article}\n");
    doc.push_str("\\usepackage[margin=1in]{geometry}\n");
    doc.push_str("\\begin{document}\n\n");

    doc.push_str(&format!("\\title{{{}}}\n", latex_escape(&data.title)));
    doc.push_str(&format!("\\author{{{}}}\n", latex_escape(&data.author)));
    doc.push_str(&format!("\\date{{{}}}\n", latex_escape(&data.date)));
    doc.push_str("\\maketitle\n\n");

    doc.push_str("\\section*{Executive Summary}\n");
    doc.push_str(&latex_escape(&data.executive_summary));
    doc.push_str("\n\n");

    for section in &data.sections {
        doc.push_str(&format!("\\section{{{}}}\n", latex_escape(&section.title)));
        doc.push_str(&latex_escape(&section.content));
        doc.push_str("\n\n");
    }

    doc.push_str("\\end{document}\n");
    doc
}

/// Render a complete LaTeX formal letter document.
pub fn render_letter(data: &LetterData) -> String {
    let mut doc = String::new();

    doc.push_str("\\documentclass{letter}\n");
    doc.push_str("\\usepackage[margin=1in]{geometry}\n");
    doc.push_str("\\begin{document}\n\n");

    doc.push_str(&format!(
        "\\address{{{} \\\\ {}}}\n",
        latex_escape(&data.from_name),
        latex_escape(&data.from_address)
    ));
    doc.push_str(&format!("\\date{{{}}}\n", latex_escape(&data.date)));

    doc.push_str(&format!(
        "\\begin{{letter}}{{{} \\\\ {}}}\n",
        latex_escape(&data.to_name),
        latex_escape(&data.to_address)
    ));
    doc.push_str(&format!(
        "\\opening{{Re: {}}}\n\n",
        latex_escape(&data.subject)
    ));
    doc.push_str(&latex_escape(&data.body));
    doc.push_str("\n\n");
    doc.push_str(&format!("\\closing{{{}}}\n", latex_escape(&data.closing)));
    doc.push_str("\\end{letter}\n\n");

    doc.push_str("\\end{document}\n");
    doc
}

/// Return metadata for all available document templates.
pub fn available_templates() -> Vec<TemplateInfo> {
    vec![
        TemplateInfo {
            name: "invoice",
            description: "Professional invoice with line items, tax, and totals",
        },
        TemplateInfo {
            name: "research_paper",
            description: "Academic paper with abstract, sections, and bibliography",
        },
        TemplateInfo {
            name: "report",
            description: "Business report with executive summary and sections",
        },
        TemplateInfo {
            name: "letter",
            description: "Formal letter with sender/recipient addresses",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invoice_template_renders() {
        let data = InvoiceData {
            invoice_number: "INV-001".into(),
            date: "2026-02-24".into(),
            from_name: "Nyaya Corp".into(),
            from_address: "123 Main St".into(),
            to_name: "Client Inc".into(),
            to_address: "456 Oak Ave".into(),
            items: vec![
                InvoiceItem {
                    description: "Consulting".into(),
                    quantity: 10.0,
                    unit_price: 150.0,
                },
                InvoiceItem {
                    description: "Development".into(),
                    quantity: 20.0,
                    unit_price: 200.0,
                },
            ],
            tax_rate: 0.10,
            notes: Some("Due in 30 days".into()),
        };
        let latex = render_invoice(&data);
        assert!(latex.contains("INV-001"));
        assert!(latex.contains("Consulting"));
        assert!(latex.contains("Nyaya Corp"));
        assert!(latex.contains("\\begin{document}"));
        assert!(latex.contains("\\end{document}"));
    }

    #[test]
    fn test_research_paper_template_renders() {
        let data = PaperData {
            title: "On Cache-First Agent Architectures".into(),
            authors: vec!["Alice Smith".into(), "Bob Jones".into()],
            abstract_text: "We present a novel approach...".into(),
            sections: vec![
                PaperSection {
                    title: "Introduction".into(),
                    content: "The problem is...".into(),
                },
                PaperSection {
                    title: "Method".into(),
                    content: "We propose...".into(),
                },
            ],
            references: vec!["Smith et al., 2025. Cache-aware routing.".into()],
        };
        let latex = render_paper(&data);
        assert!(latex.contains("On Cache-First Agent Architectures"));
        assert!(latex.contains("Alice Smith"));
        assert!(latex.contains("\\begin{abstract}"));
        assert!(latex.contains("\\section{Introduction}"));
        assert!(latex.contains("\\begin{thebibliography}"));
    }

    #[test]
    fn test_latex_escape() {
        assert_eq!(latex_escape("Price: $100 & 10%"), "Price: \\$100 \\& 10\\%");
        assert_eq!(latex_escape("file_name"), "file\\_name");
        assert_eq!(latex_escape("hash#tag"), "hash\\#tag");
    }

    #[test]
    fn test_detect_latex_backend() {
        let backend = LatexBackend::detect();
        match backend {
            LatexBackend::Tectonic(_)
            | LatexBackend::PdfLatex(_)
            | LatexBackend::XeLatex(_)
            | LatexBackend::NotFound => {}
        }
    }

    #[test]
    fn test_template_list() {
        let templates = available_templates();
        assert!(templates.len() >= 4);
        assert!(templates.iter().any(|t| t.name == "invoice"));
        assert!(templates.iter().any(|t| t.name == "research_paper"));
        assert!(templates.iter().any(|t| t.name == "report"));
        assert!(templates.iter().any(|t| t.name == "letter"));
    }
}
