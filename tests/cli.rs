use rawzip::{DataDescriptorOutput, ZipArchive, ZipArchiveWriter, path::EntryPath};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_archive-safety-check"))
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn scan(path: &Path, flags: &[&str]) -> Output {
    command().arg(path).args(flags).output().unwrap()
}

fn json_report(output: Output, exit_code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit_code),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    if exit_code == 2 {
        assert_eq!(report["status"], "incomplete");
        assert!(!report["error"].as_str().unwrap().is_empty());
    } else {
        assert_eq!(report["status"], "complete");
        assert!(report["error"].is_null());
        assert_eq!(report["entries_declared"], report["entries_inspected"]);
    }
    report
}

fn findings(report: &Value) -> Vec<(u64, &str, &str)> {
    report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            assert!(!item["reason"].as_str().unwrap().is_empty());
            (
                item["entry_index"].as_u64().unwrap(),
                item["entry"].as_str().unwrap(),
                item["rule"].as_str().unwrap(),
            )
        })
        .collect()
}

fn zip_with_names(names: &[&[u8]]) -> Vec<u8> {
    let mut writer = ZipArchiveWriter::new(Vec::new());
    for &name in names {
        let (entry, _) = writer.new_file(EntryPath::verbatim(name)).start().unwrap();
        entry.finish(DataDescriptorOutput::new(0, 0)).unwrap();
    }
    writer.finish().unwrap()
}

#[test]
fn normal_zip_and_limits_equal_to_values_exit_zero() {
    let output = scan(
        &fixture("normal.zip"),
        &[
            "--format",
            "json",
            "--max-entries",
            "2",
            "--max-file-bytes",
            "21",
            "--max-total-bytes",
            "41",
            "--max-ratio",
            "1",
        ],
    );
    let report = json_report(output, 0);
    assert_eq!(report["entries_inspected"], 2);
    assert!(findings(&report).is_empty());
}

#[test]
fn flags_parent_components_with_both_separators() {
    let output = scan(&fixture("travessia.zip"), &["--format", "json"]);
    let report = json_report(output, 1);
    assert_eq!(
        findings(&report),
        [
            (1, "../fora-da-pasta.txt", "path_traversal"),
            (2, r"docs\..\nota.txt", "path_traversal"),
        ]
    );
}

#[test]
fn flags_absolute_paths_and_drive_prefixes() {
    let output = scan(&fixture("absoluto.zip"), &["--format", "json"]);
    let report = json_report(output, 1);
    assert_eq!(
        findings(&report),
        [
            (1, "/laboratorio/absoluto.txt", "absolute_path"),
            (2, r"\laboratorio\absoluto.txt", "absolute_path"),
            (3, "C:relativo.txt", "drive_prefix"),
            (4, r"D:\laboratorio\unidade.txt", "absolute_path"),
            (4, r"D:\laboratorio\unidade.txt", "drive_prefix"),
        ]
    );
}

#[test]
fn identifies_normalized_names_and_exact_duplicates() {
    let output = scan(&fixture("colisao.zip"), &["--format", "json"]);
    let report = json_report(output, 1);
    assert_eq!(report["entries_inspected"], 4);
    assert_eq!(
        findings(&report),
        [
            (2, "docs/./nota.txt", "name_collision"),
            (3, r"docs\nota.txt", "name_collision"),
            (4, "docs/nota.txt", "name_collision"),
        ]
    );
    assert!(
        report["findings"][2]["reason"]
            .as_str()
            .unwrap()
            .contains("#1")
    );
}

#[test]
fn checks_paths_after_collapsing_components() {
    let directory = TempDir::new().unwrap();
    let zip = directory.path().join("nomes.zip");
    fs::write(
        &zip,
        zip_with_names(&[
            b"docs//./nota.txt",
            br"docs\nota.txt",
            b"docs/../docs/nota.txt",
            b"./C:relativo.txt",
            br".\D:\absoluto.txt",
            b"docs/NOTA.txt",
            b"docs/..anexo.txt",
            b"nome\0.txt",
        ]),
    )
    .unwrap();
    let report = json_report(scan(&zip, &["--format", "json"]), 1);
    assert_eq!(
        findings(&report),
        [
            (2, r"docs\nota.txt", "name_collision"),
            (3, "docs/../docs/nota.txt", "path_traversal"),
            (3, "docs/../docs/nota.txt", "name_collision"),
            (4, "./C:relativo.txt", "drive_prefix"),
            (5, r".\D:\absoluto.txt", "absolute_path"),
            (5, r".\D:\absoluto.txt", "drive_prefix"),
            (8, "nome\0.txt", "invalid_entry_name"),
        ]
    );
}

#[test]
fn identifies_symlink_by_external_attributes() {
    let output = scan(&fixture("link-simbolico.zip"), &["--format", "json"]);
    let report = json_report(output, 1);
    assert_eq!(findings(&report), [(1, "docs/atalho", "symlink")]);
}

#[test]
fn reports_all_four_limits_on_the_entry_that_exceeded_them() {
    let output = scan(
        &fixture("limites.zip"),
        &[
            "--format",
            "json",
            "--max-entries",
            "1",
            "--max-file-bytes",
            "1024",
            "--max-total-bytes",
            "1024",
            "--max-ratio",
            "10",
        ],
    );
    let report = json_report(output, 1);
    assert_eq!(report["entries_inspected"], 2);
    assert_eq!(
        findings(&report),
        [
            (2, "repeticoes.txt", "max_entries_exceeded"),
            (2, "repeticoes.txt", "max_file_bytes_exceeded"),
            (2, "repeticoes.txt", "max_total_bytes_exceeded"),
            (2, "repeticoes.txt", "max_ratio_exceeded"),
        ]
    );
}

#[test]
fn empty_zip_is_a_complete_inspection() {
    let directory = TempDir::new().unwrap();
    let zip = directory.path().join("vazio.zip");
    fs::write(&zip, zip_with_names(&[])).unwrap();
    let report = json_report(scan(&zip, &["--format", "json", "--max-entries", "0"]), 0);
    assert_eq!(report["entries_inspected"], 0);
    assert!(findings(&report).is_empty());
}

#[test]
fn truncated_zip_is_incomplete() {
    let zip = fixture("truncado.zip");
    let report = json_report(scan(&zip, &["--format", "json"]), 2);
    assert_eq!(report["entries_inspected"], 0);
    assert!(report["entries_declared"].is_null());
    assert!(findings(&report).is_empty());
    let error = report["error"].as_str().unwrap();
    assert!(error.contains("diretório central") && error.contains("truncado"));

    let text = scan(&zip, &[]);
    assert_eq!(text.status.code(), Some(2));
    assert!(
        String::from_utf8(text.stdout)
            .unwrap()
            .contains("Inspeção incompleta")
    );
}

#[test]
fn missing_input_has_a_read_error() {
    let directory = TempDir::new().unwrap();
    let missing = directory.path().join("inexistente.zip");
    let report = json_report(scan(&missing, &["--format", "json"]), 2);
    assert!(findings(&report).is_empty());
    assert!(
        report["error"]
            .as_str()
            .unwrap()
            .contains("abrir o arquivo")
    );
}

#[test]
fn preserves_earlier_findings_if_a_local_header_cannot_be_read() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("cabecalho-invalido.zip");
    let mut bytes = fs::read(fixture("travessia.zip")).unwrap();
    let archive = ZipArchive::from_slice(&bytes).unwrap();
    let mut entries = archive.entries();
    entries.next_entry().unwrap().unwrap();
    let second = entries.next_entry().unwrap().unwrap();
    let offset = usize::try_from(second.local_header_offset()).unwrap();
    bytes[offset] = 0;
    fs::write(&path, bytes).unwrap();

    let report = json_report(scan(&path, &["--format", "json"]), 2);
    assert_eq!(report["entries_declared"], 2);
    assert_eq!(report["entries_inspected"], 1);
    assert_eq!(
        findings(&report),
        [(1, "../fora-da-pasta.txt", "path_traversal")]
    );
    let error = report["error"].as_str().unwrap();
    assert!(error.contains("Entrada #2") && error.contains("cabeçalho local"));
}

#[test]
fn differing_local_name_makes_inspection_incomplete() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("nomes-divergentes.zip");
    let mut bytes = zip_with_names(&[b"doc/nota.txt"]);
    // O nome do primeiro registro começa depois dos 30 bytes fixos do cabeçalho local.
    bytes[30..33].copy_from_slice(b"../");
    fs::write(&path, bytes).unwrap();
    let report = json_report(scan(&path, &["--format", "json"]), 2);
    assert_eq!(report["entries_inspected"], 0);
    let error = report["error"].as_str().unwrap();
    assert!(error.contains("doc/nota.txt") && error.contains("..//nota.txt"));
}

#[test]
fn mismatched_entry_count_makes_inspection_incomplete() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("contagem-invalida.zip");
    let mut bytes = fs::read(fixture("normal.zip")).unwrap();
    let archive = ZipArchive::from_slice(&bytes).unwrap();
    let offset = usize::try_from(archive.eocd_offset()).unwrap();
    bytes[offset + 8..offset + 12].copy_from_slice(&[1, 0, 1, 0]);
    fs::write(&path, bytes).unwrap();
    let report = json_report(scan(&path, &["--format", "json"]), 2);
    assert_eq!(report["entries_declared"], 1);
    assert_eq!(report["entries_inspected"], 2);
    assert!(
        report["error"]
            .as_str()
            .unwrap()
            .contains("contagem declarada")
    );
}

#[test]
fn inspection_leaves_working_directory_and_parent_unchanged() {
    let directory = TempDir::new().unwrap();
    let destination = directory.path().join("destino");
    fs::create_dir(&destination).unwrap();
    let zip = destination.join("entrada.zip");
    let original = fs::read(fixture("travessia.zip")).unwrap();
    fs::write(&zip, &original).unwrap();

    let output = command()
        .current_dir(&destination)
        .args(["entrada.zip", "--format", "json"])
        .output()
        .unwrap();
    let report = json_report(output, 1);
    assert_eq!(
        findings(&report)[0],
        (1, "../fora-da-pasta.txt", "path_traversal")
    );
    assert!(!directory.path().join("fora-da-pasta.txt").exists());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
    assert_eq!(fs::read(zip).unwrap(), original);
}

#[test]
fn escapes_control_characters_in_both_output_formats() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("controle.zip");
    let name = "../nota\n\x1b[31m\u{009b}\u{202e}.txt";
    fs::write(&path, zip_with_names(&[name.as_bytes()])).unwrap();

    let text = scan(&path, &[]);
    assert_eq!(text.status.code(), Some(1));
    let stdout = String::from_utf8(text.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 5);
    assert!(stdout.contains("path_traversal") && stdout.contains(r"../nota\n"));
    assert!(!stdout.contains(['\x1b', '\u{009b}', '\u{202e}']));

    let output = scan(&path, &["--format", "json"]);
    let json = std::str::from_utf8(&output.stdout).unwrap();
    assert!(!json.contains(['\x1b', '\u{009b}', '\u{202e}']));
    assert!(json.contains(r"\u001b") && json.contains(r"\u009b"));
    let report = json_report(output, 1);
    assert_eq!(findings(&report), [(1, name, "path_traversal")]);
}

#[test]
fn does_not_decode_deflate_payload() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("deflate-invalido.zip");
    let mut bytes = fs::read(fixture("limites.zip")).unwrap();
    let archive = ZipArchive::from_slice(&bytes).unwrap();
    let mut entries = archive.entries();
    entries.next_entry().unwrap().unwrap();
    let header = entries.next_entry().unwrap().unwrap();
    let entry = archive.get_entry(header.wayfinder()).unwrap();
    let (start, _) = entry.compressed_data_range();
    // BTYPE=3 é reservado e provoca erro em um decodificador Deflate.
    bytes[usize::try_from(start).unwrap()] = 0b0000_0110;
    fs::write(&path, bytes).unwrap();

    let output = scan(&path, &["--format", "json", "--max-ratio", "1000"]);
    let report = json_report(output, 0);
    assert_eq!(report["entries_inspected"], 2);
    assert!(findings(&report).is_empty());
}
