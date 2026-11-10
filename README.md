# archive-safety-check

CLI em Rust para inspecionar metadados de arquivos ZIP antes da extração no Linux. A inspeção abre o ZIP em modo somente leitura e emite o relatório no terminal, sem descompactar conteúdo nem criar arquivos a partir das entradas.

Um extrator que junta o diretório de destino ao nome `../fora-da-pasta.txt` pode escrever na pasta pai. Por exemplo, `destino/../fora-da-pasta.txt` aponta para um arquivo ao lado de `destino/`. A ferramenta sinaliza esse padrão de travessia de diretório, conhecido como *Zip Slip* quando explorado na extração. [Pesquisa sobre Zip Slip](https://security.snyk.io/research/zip-slip-vulnerability)

## Executar no Linux

Com o Rust estável instalado via rustup, abra um terminal na raiz do repositório e compile com as versões fixadas em `Cargo.lock`. [Instalação do Rust](https://rust-lang.org/tools/install/)

```bash
cargo build --locked --release
./target/release/archive-safety-check fixtures/normal.zip
./target/release/archive-safety-check fixtures/travessia.zip
./target/release/archive-safety-check fixtures/link-simbolico.zip --format json
```

Use `--help` para consultar as opções. A saída padrão é texto. Com `--format json`, a ferramenta escreve um documento JSON em stdout, inclusive para uma inspeção incompleta. Erros de sintaxe do comando são exibidos em stderr.

| Saída do processo | Significado |
| --- | --- |
| `0` | Inspeção concluída sem achados. |
| `1` | Inspeção concluída com pelo menos um achado. |
| `2` | Erro de uso, falha ao emitir o relatório ou inspeção incompleta. |

## Regras

| Regra | O que aciona o achado |
| --- | --- |
| `path_traversal` | Componente `..` em qualquer posição. |
| `absolute_path` | Início com `/` ou `\`, ou caminho com unidade seguido de separador, como `D:\pasta`. |
| `drive_prefix` | Letra ASCII seguida de `:` no início do caminho original ou após normalizar o caminho relativo. Inclui `C:arquivo.txt`. |
| `invalid_entry_name` | Nome vazio ou com byte NUL. |
| `name_collision` | Outra entrada já aponta para o mesmo caminho relativo normalizado. O motivo identifica a primeira entrada. |
| `symlink` | Os bits de tipo Unix nos atributos externos indicam link simbólico. |
| `max_entries_exceeded` | Primeira entrada além de `--max-entries`. Padrão de 10.000 entradas. |
| `max_file_bytes_exceeded` | Entrada com tamanho descompactado declarado acima de `--max-file-bytes`. Padrão de 104.857.600 bytes (100 MiB). |
| `max_total_bytes_exceeded` | Primeira entrada que leva a soma dos tamanhos declarados acima de `--max-total-bytes`. Padrão de 1.073.741.824 bytes (1 GiB). |
| `max_ratio_exceeded` | Entrada cuja razão entre tamanhos descompactado e compactado declarados excede `--max-ratio`. Padrão de 100:1. |

O cálculo considera todos os registros do diretório central, inclusive diretórios. Os limites aceitam inteiros a partir de zero. A igualdade com o limite é permitida. Para a razão, `0/0` vale zero e um tamanho descompactado positivo com tamanho compactado zero produz um achado de taxa infinita. A leitura continua após exceder um limite.

```bash
./target/release/archive-safety-check fixtures/limites.zip --format json \
  --max-entries 1 --max-file-bytes 1024 --max-total-bytes 1024 --max-ratio 10
```

## Exemplos de relatório

Saída observada para `fixtures/travessia.zip`, com código `1`.

```text
ZIP "fixtures/travessia.zip"
Inspeção concluída
2 entradas inspecionadas (total declarado 2)
Achados (2)
- [path_traversal] #1 "../fora-da-pasta.txt" — Contém o componente '..', que pode levar a uma pasta acima do destino.
- [path_traversal] #2 "docs\\..\\nota.txt" — Contém o componente '..', que pode levar a uma pasta acima do destino.
```

JSON observado para `fixtures/link-simbolico.zip --format json`, também com código `1`.

```json
{
  "archive": "fixtures/link-simbolico.zip",
  "status": "complete",
  "entries_declared": 1,
  "entries_inspected": 1,
  "findings": [
    {
      "entry_index": 1,
      "entry": "docs/atalho",
      "rule": "symlink",
      "reason": "O tipo Unix nos atributos externos indica um link simbólico (0o120777)."
    }
  ],
  "error": null
}
```

`entry_index` começa em 1 e segue a ordem do diretório central. Em uma inspeção incompleta, `status` vale `incomplete` e `error` descreve a falha. Os achados obtidos até a interrupção permanecem no relatório. `entries_declared` vale `null` quando o leitor falha antes de obter a contagem declarada.

## Alcance das verificações

- **Metadados do ZIP.** O `rawzip` percorre o diretório central na ordem dos registros, inclusive duplicatas exatas, e fornece o nome bruto, os atributos externos e os tamanhos declarados, incluindo os campos ZIP64 que o leitor reconhece. A CLI confere o nome bruto no cabeçalho local e verifica se o intervalo declarado do conteúdo termina antes ou no início do diretório central. Também compara a contagem declarada com a lida. [Leitura de arquivos](https://docs.rs/rawzip/0.5.2/rawzip/struct.ZipArchive.html) · [Campos do diretório central](https://docs.rs/rawzip/0.5.2/rawzip/struct.ZipFileHeaderRecord.html) · [Cabeçalho local](https://docs.rs/rawzip/0.5.2/rawzip/struct.ZipEntry.html)
- **Comparação de nomes.** A normalização é lexical e diferencia maiúsculas e minúsculas. Trata `/` e `\` como separadores, elimina componentes vazios e `.` e resolve `..` quando o caminho continua dentro da raiz. Todo `..` é sinalizado. A chave de colisão é aplicada a caminhos relativos com destino dentro da raiz e sem prefixo de unidade. A comparação usa os bytes dos nomes nos cabeçalhos, sem conversão de codificações legadas, normalização Unicode ou interpretação de nomes alternativos em campos extras.
- **Exibição.** Os formatos escapam caracteres de controle. Para nomes com bytes inválidos em UTF-8, cada byte inválido aparece como `\xNN` no valor de exibição. O índice distingue registros mesmo quando os nomes exibidos coincidem.
- **Links simbólicos.** A inspeção compara os bits de tipo nos 16 bits superiores dos atributos externos com o tipo Unix de link simbólico, inclusive quando o ZIP declara outro sistema criador. É uma leitura conservadora do campo. O alvo do link fica no conteúdo da entrada e permanece fora da inspeção. [PKWARE APPNOTE, §§ 4.4.2 e 4.4.15](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT)
- **Triagem.** Os limites se baseiam nos valores declarados e sinalizam entradas para revisão. Eles não limitam o uso de CPU ou memória da inspeção. CRC, conteúdo, destinos de links, sobreposição entre entradas, ZIPs aninhados e arquivos divididos em volumes ficam fora do escopo. O código `0` indica ausência de achados nas regras executadas.

## Laboratório e desenvolvimento

Os sete arquivos em `fixtures/` são entradas de laboratório para inspeção. Use o gerador para recriá-los ou escolha outro diretório de saída. Os casos, os códigos observados e os relatórios completos estão em [Análise dos ZIPs](docs/analise-dos-zips.md).

```bash
cargo run --locked --example generate-fixtures -- fixtures
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

O fluxo [CI](.github/workflows/ci.yml) executa formatação, Clippy e testes em Linux com Rust estável. Os testes verificam as regras, as entradas e os códigos de saída, além de conferir que a inspeção deixa intactos o diretório de trabalho e sua pasta pai.

Os avisos das dependências estão em [Componentes de terceiros](THIRD_PARTY_NOTICES.md).
