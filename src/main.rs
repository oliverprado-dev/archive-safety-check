mod inspection;
mod report;

use clap::{Parser, ValueEnum};
use inspection::{Limits, inspect};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Parser)]
#[command(version, about = "Inspeciona os metadados de um ZIP antes da extração")]
struct Cli {
    /// Caminho do ZIP a inspecionar.
    archive: PathBuf,

    /// Formato do relatório.
    #[arg(long, value_enum, default_value = "text")]
    format: Format,

    /// Quantidade máxima de entradas do diretório central.
    #[arg(long, default_value_t = 10_000)]
    max_entries: u64,

    /// Tamanho descompactado máximo declarado por entrada, em bytes.
    #[arg(long, default_value_t = 104_857_600)]
    max_file_bytes: u64,

    /// Soma máxima dos tamanhos descompactados declarados, em bytes.
    #[arg(long, default_value_t = 1_073_741_824)]
    max_total_bytes: u64,

    /// Fator máximo entre tamanho descompactado e compactado (N:1).
    #[arg(long, default_value_t = 100)]
    max_ratio: u64,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let limits = Limits {
        entries: cli.max_entries,
        file_bytes: cli.max_file_bytes,
        total_bytes: cli.max_total_bytes,
        ratio: cli.max_ratio,
    };
    let report = inspect(&cli.archive, limits);
    let mut output = io::stdout().lock();
    let result = match cli.format {
        Format::Text => report.write_text(&mut output),
        Format::Json => report.write_json(&mut output),
    };

    if let Err(error) = result.and_then(|()| output.flush()) {
        eprintln!("Não foi possível escrever o relatório ({error}).");
        return ExitCode::from(2);
    }
    ExitCode::from(report.exit_code())
}
