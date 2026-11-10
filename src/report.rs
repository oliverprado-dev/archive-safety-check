use serde::Serialize;
use std::fmt::Write as _;
use std::io::{self, Write};

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Complete,
    Incomplete,
}

#[derive(Serialize)]
struct Finding {
    entry_index: u64,
    entry: String,
    rule: &'static str,
    reason: String,
}

#[derive(Serialize)]
pub struct Report {
    archive: String,
    status: Status,
    pub entries_declared: Option<u64>,
    pub entries_inspected: u64,
    findings: Vec<Finding>,
    error: Option<String>,
}

impl Report {
    pub fn new(archive: String) -> Self {
        Self {
            archive,
            status: Status::Complete,
            entries_declared: None,
            entries_inspected: 0,
            findings: Vec::new(),
            error: None,
        }
    }

    pub fn fail(&mut self, error: String) {
        self.status = Status::Incomplete;
        self.error = Some(error);
    }

    pub fn add_finding(&mut self, entry: &str, rule: &'static str, reason: impl Into<String>) {
        self.findings.push(Finding {
            entry_index: self.entries_inspected,
            entry: entry.to_owned(),
            rule,
            reason: reason.into(),
        });
    }

    pub fn exit_code(&self) -> u8 {
        match self.status {
            Status::Incomplete => 2,
            Status::Complete => u8::from(!self.findings.is_empty()),
        }
    }

    pub fn write_text(&self, mut output: impl Write) -> io::Result<()> {
        let status = match self.status {
            Status::Complete => "concluída",
            Status::Incomplete => "incompleta",
        };
        let declared = self
            .entries_declared
            .map_or_else(|| "desconhecido".into(), |count| count.to_string());
        writeln!(output, "ZIP {:?}", self.archive)?;
        writeln!(output, "Inspeção {status}")?;
        writeln!(
            output,
            "{} entradas inspecionadas (total declarado {declared})",
            self.entries_inspected
        )?;
        writeln!(output, "Achados ({})", self.findings.len())?;
        for finding in &self.findings {
            writeln!(
                output,
                "- [{}] #{} {:?} — {}",
                finding.rule, finding.entry_index, finding.entry, finding.reason
            )?;
        }
        if let Some(error) = &self.error {
            writeln!(output, "Erro — {error}")?;
        }
        Ok(())
    }

    pub fn write_json(&self, mut output: impl Write) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let mut start = 0;
        for (offset, matched) in json.match_indices(escape_in_json) {
            output.write_all(&json.as_bytes()[start..offset])?;
            let codepoint = u32::from(matched.chars().next().expect("um caractere"));
            write!(output, "\\u{codepoint:04x}")?;
            start = offset + matched.len();
        }
        writeln!(output, "{}", &json[start..])
    }
}

fn escape_in_json(character: char) -> bool {
    // O serializador já escapa os controles ASCII abaixo de U+0020 nos campos.
    (character.is_control() && character != '\n')
        || matches!(
            character,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

pub fn display_name(raw: &[u8]) -> String {
    let mut name = String::new();
    for chunk in raw.utf8_chunks() {
        name.push_str(chunk.valid());
        for byte in chunk.invalid() {
            write!(name, "\\x{byte:02x}").expect("escrever em String");
        }
    }
    name
}
