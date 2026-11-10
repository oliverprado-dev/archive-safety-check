use crate::report::{Report, display_name};
use rawzip::{FileReader, RECOMMENDED_BUFFER_SIZE, ZipArchive, ZipFileHeaderRecord};
use std::collections::{HashMap, hash_map::Entry};
use std::fs::OpenOptions;
use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};
use std::path::Path;

const MAX_HEADER_FIELD_LEN: usize = u16::MAX as usize;
const UNIX_FILE_TYPE_MASK: u32 = 0o170000;
const UNIX_SYMLINK_TYPE: u32 = 0o120000;

pub struct Limits {
    pub entries: u64,
    pub file_bytes: u64,
    pub total_bytes: u64,
    pub ratio: u64,
}

pub fn inspect(path: &Path, limits: Limits) -> Report {
    let mut report = Report::new(display_name(path.as_os_str().as_bytes()));
    if let Err(error) = inspect_zip(path, &limits, &mut report) {
        report.fail(error);
    }
    report
}

fn inspect_zip(path: &Path, limits: &Limits, report: &mut Report) -> Result<(), String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| format!("Não foi possível abrir o arquivo ({error})."))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Não foi possível consultar o arquivo ({error})."))?;
    if !metadata.is_file() {
        return Err("O caminho precisa apontar para um arquivo regular.".into());
    }

    let mut locator_buffer = vec![0; RECOMMENDED_BUFFER_SIZE];
    let archive = ZipArchive::from_file(file, &mut locator_buffer).map_err(|error| {
        format!(
            "Não foi possível ler o fim do diretório central do ZIP ({error}). \
             O arquivo pode estar inválido ou truncado."
        )
    })?;
    let declared = archive.entries_hint();
    report.entries_declared = Some(declared);
    if archive.end_offset() > metadata.len() {
        return Err("O fim declarado do ZIP fica além do tamanho do arquivo.".into());
    }

    // O diretório central tem três campos de comprimento variável de até u16::MAX bytes.
    // O cabeçalho local tem dois.
    let mut directory_buffer = vec![0; 3 * MAX_HEADER_FIELD_LEN];
    let mut local_buffer = vec![0; 2 * MAX_HEADER_FIELD_LEN];
    let mut entries = archive.entries(&mut directory_buffer);
    let mut seen = HashMap::new();
    let mut total = 0_u128;
    loop {
        let index = report.entries_inspected + 1;
        let Some(entry) = entries.next_entry().map_err(|error| {
            format!("Não foi possível ler a entrada #{index} do diretório central ({error}).")
        })?
        else {
            break;
        };
        let raw_name = entry.file_path().as_bytes();
        let name = display_name(raw_name);
        validate_local_header(&archive, &entry, &mut local_buffer)
            .map_err(|error| format!("Entrada #{index} {name:?} — {error}"))?;

        report.entries_inspected = index;
        inspect_path(raw_name, &name, &mut seen, report);

        let unix_mode = entry.external_attributes() >> 16;
        if unix_mode & UNIX_FILE_TYPE_MASK == UNIX_SYMLINK_TYPE {
            report.add_finding(
                &name,
                "symlink",
                format!(
                    "O tipo Unix nos atributos externos indica um link simbólico ({unix_mode:#o})."
                ),
            );
        }
        inspect_limits(&entry, &name, limits, &mut total, report);
    }

    if report.entries_inspected != declared {
        return Err(format!(
            "A contagem declarada ({declared}) diverge das {} entradas lidas no diretório central.",
            report.entries_inspected
        ));
    }
    Ok(())
}

fn validate_local_header(
    archive: &ZipArchive<FileReader>,
    central: &ZipFileHeaderRecord<'_>,
    buffer: &mut [u8],
) -> Result<(), String> {
    let entry = archive.get_entry(central.wayfinder()).map_err(|error| {
        format!("Não foi possível localizar os dados pelo cabeçalho local ({error}).")
    })?;
    let (_, end) = entry.compressed_data_range();
    if end > archive.directory_offset() {
        return Err("O intervalo declarado ultrapassa o início do diretório central.".into());
    }
    let local = entry
        .local_header(buffer)
        .map_err(|error| format!("Não foi possível ler o cabeçalho local ({error})."))?;
    let local_name = local.file_path().as_bytes();
    if local_name != central.file_path().as_bytes() {
        return Err(format!(
            "O nome no cabeçalho local ({:?}) difere do nome no diretório central.",
            display_name(local_name)
        ));
    }
    Ok(())
}

struct SeenName {
    index: u64,
    name: String,
}

fn is_separator(byte: u8) -> bool {
    matches!(byte, b'/' | b'\\')
}

fn has_drive_prefix(path: &[u8]) -> bool {
    matches!(path, [letter, b':', ..] if letter.is_ascii_alphabetic())
}

fn is_absolute(path: &[u8]) -> bool {
    path.first().copied().is_some_and(is_separator)
        || (has_drive_prefix(path) && path.get(2).copied().is_some_and(is_separator))
}

fn relative_key(path: &[u8]) -> Option<Vec<u8>> {
    let mut components = Vec::new();
    for component in path.split(|&byte| is_separator(byte)) {
        match component {
            b"" | b"." => {}
            b".." => {
                components.pop()?;
            }
            _ => components.push(component),
        }
    }
    (!components.is_empty()).then(|| components.join(&b'/'))
}

fn inspect_path(
    raw: &[u8],
    name: &str,
    seen: &mut HashMap<Vec<u8>, SeenName>,
    report: &mut Report,
) {
    let invalid_name = raw.is_empty() || raw.contains(&0);
    let key = if invalid_name || is_absolute(raw) || has_drive_prefix(raw) {
        None
    } else {
        relative_key(raw)
    };
    let normalized = key.as_deref().unwrap_or(raw);
    let absolute = is_absolute(raw) || is_absolute(normalized);
    let drive = has_drive_prefix(raw) || has_drive_prefix(normalized);

    if raw
        .split(|&byte| is_separator(byte))
        .any(|part| part == b"..")
    {
        report.add_finding(
            name,
            "path_traversal",
            "Contém o componente '..', que pode levar a uma pasta acima do destino.",
        );
    }
    if absolute {
        report.add_finding(
            name,
            "absolute_path",
            "O caminho absoluto pode levar a uma gravação fora do destino.",
        );
    }
    if drive {
        report.add_finding(
            name,
            "drive_prefix",
            "Começa com um prefixo de unidade, como C:.",
        );
    }
    if invalid_name {
        report.add_finding(
            name,
            "invalid_entry_name",
            "O nome está vazio ou contém um byte NUL.",
        );
    }
    if let Some(key) = key.filter(|_| !absolute && !drive) {
        match seen.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(SeenName {
                    index: report.entries_inspected,
                    name: name.to_owned(),
                });
            }
            Entry::Occupied(slot) => {
                let first = slot.get();
                report.add_finding(
                    name,
                    "name_collision",
                    format!(
                        "Normaliza para {:?}, assim como a entrada #{} {:?}.",
                        display_name(slot.key()),
                        first.index,
                        first.name
                    ),
                );
            }
        }
    }
}

fn inspect_limits(
    entry: &ZipFileHeaderRecord<'_>,
    name: &str,
    limits: &Limits,
    total: &mut u128,
    report: &mut Report,
) {
    let size = entry.uncompressed_size_hint();
    let compressed = entry.compressed_size_hint();
    let previous_total = *total;
    *total += u128::from(size);

    if report.entries_inspected - 1 == limits.entries {
        report.add_finding(
            name,
            "max_entries_exceeded",
            format!("É a primeira entrada além do limite de {}.", limits.entries),
        );
    }
    if size > limits.file_bytes {
        report.add_finding(
            name,
            "max_file_bytes_exceeded",
            format!(
                "Declara {size} bytes descompactados, acima do limite de {} bytes.",
                limits.file_bytes
            ),
        );
    }
    let total_limit = u128::from(limits.total_bytes);
    if previous_total <= total_limit && *total > total_limit {
        report.add_finding(
            name,
            "max_total_bytes_exceeded",
            format!(
                "A soma declarada chegou a {total} bytes descompactados, acima do limite de {total_limit} bytes."
            ),
        );
    }
    if u128::from(size) > u128::from(compressed) * u128::from(limits.ratio) {
        let reason = if compressed == 0 {
            format!("Declara {size} bytes descompactados com 0 bytes compactados (taxa infinita).")
        } else {
            format!(
                "A taxa declarada de {size}/{compressed} supera o limite de {}:1.",
                limits.ratio
            )
        };
        report.add_finding(name, "max_ratio_exceeded", reason);
    }
}
