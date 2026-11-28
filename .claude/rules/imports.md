---
paths:
  - "**/*.rs"
---

# use 文規約

`use` 文は次の順序で並べ、**隣接するグループの間に空行を1行入れる。** 存在しないグループは省略する。

1. `std::` — 標準ライブラリ
2. 外部クレート
3. クレート内部 — `self::` → `super::` → `crate::` → 同じモジュールで `mod` 宣言した子モジュール (`use args::...` など) の順

グループ内では空行を入れない。並びは rustfmt に従う。グループ 1・2 はパス全体のアルファベット順、グループ 3 は上記の接頭辞順で、同じ接頭辞の中はアルファベット順になる (rustfmt が空行のない範囲を自動で整列するため、これと違う順にすると `cargo fmt --check` が失敗する)。`use` ブロックの直後にも空行を1行入れてから次のコード (セクション区切り・定義など) を書く。

`lib.rs` では `mod` 宣言を `use` より前に置く。`main.rs` はバイナリのエントリポイントのみとする。

```rust
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use crate::app::state::{App, DialogResult, IoMsg};
use crate::image_output::{self, OutputFormat, SaveOptions};
use crate::trim::apply_trim;

// ======================================================================
// 保存
// ======================================================================
```

```rust
use std::sync::mpsc;

use rayon::prelude::*;
use serde::Deserialize;

use super::state::App;
```

## 書かないこと

- グループをまたいだ空行の欠如、またはグループ内の不要な空行
- `super` / `crate` を外部クレートより前に置く並び
- グループ内のアルファベット順でない並び
