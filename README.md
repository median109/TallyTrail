# TallyTrail

TallyTrail 是一个**本地优先、单文件存储**的记账与资产管理工具。第一阶段（M1）交付纯 Rust Core + 命令行界面（CLI）：

- 所有账本数据保存在一个 SQLite 文件中，不依赖服务器、账号或网络。
- `Core` 只包含业务逻辑与数据访问，不依赖任何 UI 库。
- `CLI` 只通过 Core 暴露的 API 操作数据，不直接拼 SQL。
- 金额一律使用 `i64` 分，时间一律使用 UTC 秒时间戳，ID 为 UUID v4。

## 文档

| 文档 | 内容 |
| --- | --- |
| [核心使用教程](docs/核心使用教程.md) | Core 的 API、数据模型、三套统计口径、退款/报销、贷款、预算与 WAL 注意事项 |
| [CLI 使用教程](docs/CLI使用教程.md) | 从零开始建账、全部命令示例、日期/金额规则、常见错误 |
| [项目要求](项目要求.md) | 第一阶段需求原文 |

## 目录结构

```text
TallyTrail/
├─ Cargo.toml            # Workspace（resolver 2）
├─ Core/                 # tallytrail-core：纯业务核心
│  ├─ src/
│  │  ├─ api/core.rs     # CoreHandle：唯一对外入口
│  │  ├─ data/           # SQLite 连接、迁移、repository
│  │  └─ domain/         # DTO、枚举、校验规则、错误类型
│  ├─ tests/             # 业务规则集成测试
│  └─ examples/          # 可运行的 API 快速上手示例
├─ CLI/                  # tallytrail-cli：命令行
│  └─ src/
│     ├─ main.rs         # clap 命令定义
│     ├─ commands.rs     # 命令实现（仅调用 Core API）
│     └─ util.rs         # 金额/日期解析与表格输出
└─ docs/                 # 教程
```

## 构建

需要 Rust 1.85+（开发时使用 1.99.1）与 C 工具链（`rusqlite` 使用 bundled SQLite）。

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
```

生成的 CLI 可执行文件位于 `target/debug/tallytrail`（Windows 为 `target\debug\tallytrail.exe`）。

## 五分钟上手

```bash
# 1. 建账户
tallytrail --db my.db account add --name 招商银行 --type bank --initial 10000.00
tallytrail --db my.db account add --name 支付宝   --type e-wallet --initial 500.00

# 2. 建分类（两级）
tallytrail --db my.db category add --name 餐饮 --kind expense
tallytrail --db my.db category add --name 午餐 --kind expense --parent <餐饮ID>

# 3. 记一笔支出
tallytrail --db my.db add-expense --at "2026-10-01 12:30" \
  --category <午餐ID> --amount 45.50 --pay <支付宝ID>=45.50 --note 工作日午餐

# 4. 看余额与流水
tallytrail --db my.db balance
tallytrail --db my.db list --from 2026-10-01 --to 2026-10-31
```

完整流程见 [CLI 使用教程](docs/CLI使用教程.md)。

## 核心设计要点

### 金额

- 单位固定为**分**（`i64`），例如 `45.50` 元存为 `4550`。
- 所有累加/累减都经过 `checked_add` / `checked_sub`，溢出返回 `CoreError::AmountOverflow`。
- CLI 输入金额为十进制字符串，最多两位小数，内部不使用浮点数；展示时统一格式化为两位小数。

### 时间

- 存储与 API 均使用 **UTC 秒时间戳**（`i64`）。
- CLI 接受 `YYYY-MM-DD HH:MM`：按**本地时区**解释后转为 UTC。
- CLI 也接受 `YYYY-MM-DD`：按 **UTC 自然日**解释，保证月/周时间桶与数据库边界一致；展示为日期本身。
- 统计范围统一为左闭右开 `[from, to)`；CLI 的 `--to` 若只给日期，会自动包含整日。

### 多币种与双金额

每笔交易同时保留两组金额：

| 字段 | 含义 |
| --- | --- |
| `total_amount_cents` / `currency` | 花费币种口径（商品原价 / 应报销金额） |
| `actual_amount_cents` | 实际结算金额，按**结算账户币种** |

- 花费币种与结算账户币种相同时，`actual` 可省略，默认等于 `total`。
- 币种不同时必须显式给出 `actual`，否则返回校验错误。
- 同一笔交易的所有结算账户币种必须一致，否则 `CoreError::MixedCurrencySettlement`。
- 系统不做汇率换算，也不合并不同币种；所有统计都按币种分组。

### 三套统计口径

| 口径 | 数据来源 | 用途 |
| --- | --- | --- |
| 花费币种 | `total_amount_cents` + `currency` | 预算、分类支出、消费分析 |
| 实际结算 | `actual_amount_cents` + 结算账户币种 | 账户余额、净资产 |
| 现金流 | 结算行的 `role` 与方向 | 收入/支出/退款/报销/转账/贷款 |

退款与报销会继承原交易的**花费币种**，并以负向金额冲减分类支出；但它们的实际入账金额仍按结算账户币种记录。

### 结算角色

| 角色 | 交易类型 | 方向 |
| --- | --- | --- |
| `PAYMENT` | 支出、贷款还款 | 资金流出 |
| `RECEIPT` | 收入、退款、贷款放款 | 资金流入 |
| `REIMBURSE_SOURCE` | 报销 | 资金流入（来源账户） |

结算行金额必须为正数，且同一交易内角色组合与金额合计必须匹配，否则拒绝写入。

### 分类与标签

- 分类最多两级；二级分类的 `kind` 必须与父分类一致。
- 已产生交易或预算引用的分类不能删除，也不能修改 `kind`（返回 `CategoryInUse`）。
- 分类可停用：停用后不能用于新交易，但历史数据保留。
- 标签名称大小写不敏感唯一；删除标签只解除关联，不影响交易。

### 退款、报销与作废

- 退款/报销必须指向一条**未作废的 `EXPENSE`**。
- 单笔及累计退款不能超过原支出；报销同理。
- 原支出存在未作废的退款/报销时，不能作废该支出（`HasActiveDependents`）。
- 贷款放款存在未作废的还款时，不能作废放款。
- 作废是软删除：交易保留，状态改为 `VOIDED`，并记录作废时间与原因；所有统计只计算 `CONFIRMED`。

### 贷款

- 贷款记录本金、币种、年化基点（`450` = 4.50%）、起始时间、期限与关联账户。
- 放款生成 `LOAN_DISBURSE`，计入账户资产，但不计入收入。
- 还款拆分为本金与利息：本金冲减未还本金，利息计入支出。
- 未还本金 = 本金 − 累计已还本金；净资产中作为该币种负债扣除。

### 预算

- 预算范围：`TOTAL`、`CATEGORY_L1`、`CATEGORY_L2`。
- 周期：`MONTHLY`、`YEARLY`、`CUSTOM`（必须同时给出起止时间）。
- 一级分类预算自动包含其下所有二级分类。
- 预算只统计当前周期内、与预算币种相同、状态为 `CONFIRMED` 的净支出（支出 − 退款 − 报销）。
- 贷款相关交易（`LOAN_*`，含本金与利息）不计入预算；利息仍计入收支汇总与分类支出序列。

### 净资产

净资产按币种分组：

```text
净资产 = 账户余额合计 − 贷款未还本金
```

- 账户余额 = 初始余额 + 所有已确认结算行（按角色加减）。
- 未来起始的贷款不计入历史时点。
- 转账是内部流动，不改变净资产。
- 不做跨币种汇率折算。

## 数据文件与备份

默认数据库为当前目录下的 `tallytrail.db`，可用 `--db <path>` 指定其他路径。

数据库启用 WAL 模式。**直接拷贝 `.db` 文件前必须先 checkpoint 或正常关闭程序**，否则最近的写入可能仍在 `-wal` 文件中：

```bash
tallytrail --db my.db checkpoint
```

CLI 每次命令结束会自动 checkpoint；`CoreHandle` 在最后一个句柄析构时也会尝试 checkpoint。

## 测试

```bash
cargo test --workspace
```

集成测试覆盖：组合支付、境外双金额、退款/报销继承与级联、作废拦截、贷款生命周期、预算口径、净资产、时间桶、分类树、标签大小写、金额溢出、checkpoint 后重开等。

运行 Core 示例：

```bash
cargo run -p tallytrail-core --example quickstart
```

## 第一阶段范围

已实现：本地 SQLite 单文件、Core API、CLI 全部命令、三套口径、多币种双金额、退款/报销、贷款、预算、净资产、checkpoint。

暂不包含：Android/FFI/Web 层、汇率换算、多用户/同步、加密、导入导出、图形界面。
