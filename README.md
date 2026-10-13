# coax-rs

`coaxd`をRustから使うためのクライアントと、放送ストリームをパースするライブラリ実装。

`coaxd`はチューナーを持つデーモンです（[coax](https://github.com/Till0196/coax)）。
チューナーを使うプログラムは、ボードを直接開かずに`coaxd`に要求します。
このリポジトリは、その要求の方法と、返されたストリームを読む機能を提供します。

| クレート | 中身 |
| --- | --- |
| `coax` | `coaxd`クライアント |
| `mpegts` | TSパケット、セクションの組み立て、CRC、PAT |
| `mmttlv` | TLVパケット、MMTPヘッダー、MMTのシグナリング |
| `isdb` | ARIBのテーブル（NIT、SDT、TSMFなど）と文字符号 |

### ビルド

```sh
cargo nextest run          # テスト（cargo-nextest）
cargo test --doc           # doctest は nextest では走らない
cargo clippy --all-targets
cargo fmt --all
```

`cargo-nextest`は`curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C ~/.cargo/bin`で入ります（macOSは`latest/mac`）。
設定は`.config/nextest.toml`にあります。

Windows向けの型検査はリンクしないので、ツールチェーン無しでできます。

```sh
cargo check --target x86_64-pc-windows-gnullvm
```

### 注意

本ライブラリには CAS（限定受信方式）の処理は含まれていません。
暗号化された放送は再生できません。

### ライセンス

Apache-2.0です。
全文は[LICENSE](LICENSE)にあります。
