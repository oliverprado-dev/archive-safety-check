# Análise dos ZIPs de laboratório

Os resultados abaixo foram observados em Linux x86_64 com Rust estável 1.99.1. Execute os comandos a partir da raiz do repositório. Cada link na coluna de resultado aponta para o JSON emitido naquela execução. O código mostrado é o código de saída do processo.

```bash
cargo build --locked --release
check=./target/release/archive-safety-check
```

| Arquivo | Comando usado | Resultado observado | Interpretação |
| --- | --- | --- | --- |
| `normal.zip` | `"$check" fixtures/normal.zip --format json` | `0` · 2 entradas inspecionadas e nenhum achado. [JSON](relatorios/normal.json) | Os dois nomes são relativos e distintos. Os tamanhos declarados ficaram dentro dos limites padrão. |
| `travessia.zip` | `"$check" fixtures/travessia.zip --format json` | `1` · `path_traversal` em `../fora-da-pasta.txt` e `docs\..\nota.txt`. [JSON](relatorios/travessia.json) | A primeira entrada pode alcançar a pasta pai do destino. A segunda mostra que o componente `..` também é sinalizado com barras invertidas. |
| `absoluto.zip` | `"$check" fixtures/absoluto.zip --format json` | `1` · 3 achados `absolute_path` e 2 `drive_prefix` em 4 entradas. [JSON](relatorios/absoluto.json) | As barras iniciais marcam caminhos absolutos. `C:relativo.txt` tem prefixo de unidade e `D:\laboratorio\unidade.txt` aciona as duas regras. |
| `colisao.zip` | `"$check" fixtures/colisao.zip --format json` | `1` · 3 achados `name_collision` nas entradas #2, #3 e #4. [JSON](relatorios/colisao.json) | `docs/./nota.txt`, `docs\nota.txt` e a duplicata exata `docs/nota.txt` disputam o mesmo destino com a entrada #1. A ordem de extração pode determinar qual conteúdo fica no disco. |
| `link-simbolico.zip` | `"$check" fixtures/link-simbolico.zip --format json` | `1` · `symlink` em `docs/atalho`. [JSON](relatorios/link-simbolico.json) | O modo Unix declarado é `0o120777`. Os bits de tipo `0o120000` identificam o link. |
| `limites.zip` | `"$check" fixtures/limites.zip --format json --max-entries 1 --max-file-bytes 1024 --max-total-bytes 1024 --max-ratio 10` | `1` · Os quatro limites foram excedidos em `repeticoes.txt` (#2). [JSON](relatorios/limites.json) | A segunda entrada declara 2.048 bytes descompactados e 25 compactados. A soma chega a 2.060 bytes, e a razão declarada de 81,92:1 supera o limite de 10:1. |
| `truncado.zip` | `"$check" fixtures/truncado.zip --format json` | `2` · `incomplete`, nenhuma entrada inspecionada e falha ao ler o fim do diretório central. [JSON](relatorios/truncado.json) | O registro final está cortado. A lista vazia de achados acompanha uma falha de leitura. |

## O que fazer com um caminho suspeito

1. Interrompa a extração automática e preserve o ZIP original. Registre a origem, o SHA-256 do arquivo, o índice da entrada e o relatório JSON.
2. Calcule o destino pretendido pelo extrator sem gravar a entrada. Com destino em `/tmp/triagem/destino/`, a junção com `../fora-da-pasta.txt` resolve para `/tmp/triagem/fora-da-pasta.txt`, fora da raiz autorizada.
3. Verifique o extrator e a política que seriam usados. Se o ZIP já tiver sido processado, examine o destino e as pastas alcançáveis pelo caminho, correlacione horários e compare arquivos potencialmente alterados com uma referência conhecida.
4. Caso a análise do conteúdo seja necessária, use um ambiente isolado com diretório vazio, usuário sem privilégios e cotas de recursos. A política de extração deve validar o destino efetivo de cada entrada e rejeitar travessia, colisões e links que possam redirecionar gravações.

O teste de integração que usa `travessia.zip` executou a CLI com `destino/` como diretório de trabalho. Ao final, o ZIP tinha os mesmos bytes, `destino/` só continha o ZIP, e a pasta pai só continha `destino/`.
