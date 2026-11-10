use clap::Parser;
use flate2::{Compression, write::DeflateEncoder};
use rawzip::{CompressionMethod, ZipArchiveWriter, path::EntryPath, time::UtcDateTime};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Grava os ZIPs pequenos do laboratório")]
struct Cli {
    #[arg(default_value = "fixtures")]
    directory: PathBuf,
}

enum Kind {
    Stored,
    Deflated,
    Symlink,
}

struct LabEntry<'a> {
    name: &'a str,
    contents: &'a [u8],
    kind: Kind,
}

impl<'a> LabEntry<'a> {
    fn stored(name: &'a str, contents: &'a [u8]) -> Self {
        Self {
            name,
            contents,
            kind: Kind::Stored,
        }
    }
}

fn make_zip(entries: &[LabEntry<'_>]) -> Result<Vec<u8>, rawzip::Error> {
    let mut archive = ZipArchiveWriter::new(Vec::new());
    let date = UtcDateTime::from_unix(1_704_067_200);
    for entry in entries {
        // O modo verbatim preserva barras, componentes '.' e nomes repetidos.
        let builder = archive
            .new_file(EntryPath::verbatim(entry.name.as_bytes()))
            .last_modified(date);
        match entry.kind {
            Kind::Stored | Kind::Symlink => {
                let mode = match entry.kind {
                    Kind::Symlink => 0o120777,
                    _ => 0o100644,
                };
                let (mut file, config) = builder.unix_permissions(mode).start()?;
                let mut writer = config.wrap(&mut file);
                writer.write_all(entry.contents)?;
                let (_, descriptor) = writer.finish()?;
                file.finish(descriptor)?;
            }
            Kind::Deflated => {
                let (mut file, config) = builder
                    .compression_method(CompressionMethod::DEFLATE)
                    .start()?;
                let encoder = DeflateEncoder::new(&mut file, Compression::default());
                let mut writer = config.wrap(encoder);
                writer.write_all(entry.contents)?;
                let (encoder, descriptor) = writer.finish()?;
                encoder.finish()?;
                file.finish(descriptor)?;
            }
        }
    }
    archive.finish()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = Cli::parse().directory;
    fs::create_dir_all(&directory)?;

    let repeated = [b'A'; 2048];
    let normal = make_zip(&[
        LabEntry::stored("docs/nota.txt", b"Nota de laboratorio.\n"),
        LabEntry::stored("leia-me.txt", b"Arquivo de exemplo.\n"),
    ])?;
    let truncated = &normal[..normal.len() - 12];
    let labs = [
        (
            "travessia.zip",
            make_zip(&[
                LabEntry::stored("../fora-da-pasta.txt", b"Fora do destino.\n"),
                LabEntry::stored(
                    r"docs\..\nota.txt",
                    b"Componente pai com barras invertidas.\n",
                ),
            ])?,
        ),
        (
            "absoluto.zip",
            make_zip(&[
                LabEntry::stored("/laboratorio/absoluto.txt", b"Caminho com barra normal.\n"),
                LabEntry::stored(
                    r"\laboratorio\absoluto.txt",
                    b"Caminho com barra invertida.\n",
                ),
                LabEntry::stored("C:relativo.txt", b"Unidade com caminho relativo.\n"),
                LabEntry::stored(
                    r"D:\laboratorio\unidade.txt",
                    b"Unidade com caminho absoluto.\n",
                ),
            ])?,
        ),
        (
            "colisao.zip",
            make_zip(&[
                LabEntry::stored("docs/nota.txt", b"Primeira entrada.\n"),
                LabEntry::stored("docs/./nota.txt", b"Ponto no caminho.\n"),
                LabEntry::stored(r"docs\nota.txt", b"Barra invertida.\n"),
                LabEntry::stored("docs/nota.txt", b"Mesmo nome da primeira.\n"),
            ])?,
        ),
        (
            "link-simbolico.zip",
            make_zip(&[LabEntry {
                name: "docs/atalho",
                contents: b"../../fora-da-pasta.txt",
                kind: Kind::Symlink,
            }])?,
        ),
        (
            "limites.zip",
            make_zip(&[
                LabEntry::stored("indice.txt", b"Laboratorio\n"),
                LabEntry {
                    name: "repeticoes.txt",
                    contents: &repeated,
                    kind: Kind::Deflated,
                },
            ])?,
        ),
    ];

    for (name, bytes) in labs {
        fs::write(directory.join(name), bytes)?;
    }
    fs::write(directory.join("normal.zip"), &normal)?;
    fs::write(directory.join("truncado.zip"), truncated)?;
    println!("7 ZIPs gravados em {:?}", directory);
    Ok(())
}
