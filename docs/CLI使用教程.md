# TallyTrail CLI 使用教程

CLI 是 TallyTrail 第一阶段的交互界面，所有业务规则仍然在 Core 中。本文从零开始演示如何建账、记流水、处理退款报销、贷款、预算与统计。

## 1. 构建与运行

```bash
cargo build --workspace
```

可执行文件：

- Linux / macOS：`target/debug/tallytrail`
- Windows：`target\debug\tallytrail.exe`

直接运行：

```bash
tallytrail --help
tallytrail --db my.db account list
```

也可以不安装，用 `cargo run`：

```bash
cargo run -p tallytrail-cli -- --db my.db account list
```

### 数据库路径

`--db <path>` 是全局参数，默认 `tallytrail.db`（当前目录）。建议放在固定位置：

```bash
tallytrail --db ~/Documents/tallytrail.db balance
```

首次运行会自动创建数据库并执行迁移。

## 2. 日期与金额规则

### 日期

| 输入 | 含义 |
| --- | --- |
| `2026-10-01 12:30` | 本地时间，转换为 UTC 存储 |
| `2026-10-01` | UTC 自然日零点；展示为日期本身 |
| `--from 2026-10-01` | 范围起点，含当天 00:00（UTC） |
| `--to 2026-10-31` | 范围终点，含当天整天（UTC 次日 00:00，左闭右开） |
| `--as-of 2026-10-31` | 截止到当天最后一秒 |

时间范围统一为左闭右开 `[from, to)`。

### 金额

- 十进制字符串，最多两位小数：`45.50`、`0.01`、`100`。
- 不接受浮点误差、科学计数法或超过两位小数。
- 展示统一保留两位小数。
- 金额单位是“分”，但 CLI 输入输出都用“元”。

## 3. 五分钟快速开始

以下命令中的 `<...ID>` 需要替换成上一条命令输出的 UUID。

### 3.1 建账户

```bash
tallytrail --db my.db account add --name 招商银行 --type bank --initial 10000.00
# 账户已创建: 4e50d283-...

tallytrail --db my.db account add --name 支付宝 --type e-wallet --initial 500.00
# 账户已创建: 6796b27f-...
```

账户类型：`bank`、`cash`、`e-wallet`、`credit`、`reimburse-source`、`other`。

信用卡欠款可以用负数初始余额：

```bash
tallytrail --db my.db account add --name 信用卡 --type credit --initial -2000.00
```

查看：

```bash
tallytrail --db my.db account list
tallytrail --db my.db account list --all   # 含停用账户
```

### 3.2 建分类

```bash
tallytrail --db my.db category add --name 餐饮 --kind expense
# 分类已创建: 9bc8202f-...

tallytrail --db my.db category add --name 午餐 --kind expense --parent 9bc8202f-...
# 分类已创建: 9cc26125-...

tallytrail --db my.db category add --name 工资 --kind income
```

分类最多两级；二级分类的 `kind` 必须与父级一致。

### 3.3 建标签

```bash
tallytrail --db my.db tag add 出差
# 标签已创建: 27380fbd-...
```

### 3.4 记支出

```bash
tallytrail --db my.db add-expense \
  --at "2026-10-01 12:30" \
  --category 9cc26125-... \
  --amount 45.50 \
  --pay 6796b27f-...=45.50 \
  --note 工作日午餐 \
  --tag 27380fbd-...
# 已创建支出: a60daa40-...
```

- `--pay` 格式为 `账户ID=金额`，可重复。
- `--amount` 是花费金额；`--actual` 是实际结算金额（可选）。
- `--currency` 默认 `CNY`。

### 3.5 查看余额与流水

```bash
tallytrail --db my.db balance
```

```text
账户 ID                               名称      类型      币种  余额
------------------------------------  --------  --------  ----  -------
4e50d283-...                          招商银行  BANK      CNY   10000.00
6796b27f-...                          支付宝    E_WALLET  CNY   454.50

资产合计（按币种，不含贷款负债）: CNY 10454.50
```

```bash
tallytrail --db my.db list --from 2026-10-01 --to 2026-10-31
```

## 4. 账户管理

```bash
# 新建
tallytrail --db my.db account add --name 现金 --type cash --initial 1000

# 列表（默认只显示启用账户）
tallytrail --db my.db account list
tallytrail --db my.db account list --all

# 重命名
tallytrail --db my.db account rename <账户ID> 新名称

# 停用 / 启用
tallytrail --db my.db account disable <账户ID>
tallytrail --db my.db account enable <账户ID>
```

停用后该账户不能用于新交易，历史数据仍参与统计和余额计算。

账户没有物理删除命令，避免破坏历史流水。

## 5. 分类管理

```bash
# 新建一级分类
tallytrail --db my.db category add --name 交通 --kind expense

# 新建二级分类
tallytrail --db my.db category add --name 地铁 --kind expense --parent <一级分类ID>

# 列表
tallytrail --db my.db category list
tallytrail --db my.db category list --kind expense
tallytrail --db my.db category list --all

# 重命名 / 停用 / 启用
tallytrail --db my.db category rename <分类ID> 新名称
tallytrail --db my.db category disable <分类ID>
tallytrail --db my.db category enable <分类ID>

# 修改 kind（有交易、预算或子分类时会被拒绝）
tallytrail --db my.db category set-kind <分类ID> --kind income

# 物理删除（无子分类且无任何引用时）
tallytrail --db my.db category delete <分类ID>
```

常见错误：

```text
错误: 校验失败: 分类 kind 与交易类型不匹配
错误: 分类已被引用或仍有子分类，不能执行该操作
错误: 校验失败: 二级分类的 kind 必须与父分类一致
```

## 6. 标签管理

```bash
tallytrail --db my.db tag add 旅行
tallytrail --db my.db tag list
tallytrail --db my.db tag rename <标签ID> 差旅
tallytrail --db my.db tag delete <标签ID>
```

标签名称大小写不敏感唯一。删除标签只解除交易关联，不删除交易。

## 7. 记支出

### 7.1 单账户

```bash
tallytrail --db my.db add-expense \
  --at "2026-10-01 12:30" \
  --category <分类ID> \
  --amount 45.50 \
  --pay <账户ID>=45.50
```

### 7.2 组合支付

```bash
tallytrail --db my.db add-expense \
  --at "2026-10-02 18:00" \
  --category <分类ID> \
  --amount 455.00 \
  --pay <银行卡ID>=300.00 \
  --pay <支付宝ID>=155.00
```

所有结算账户必须同币种，金额合计必须等于实际金额。

### 7.3 境外消费（双金额）

```bash
tallytrail --db my.db add-expense \
  --at "2026-10-02 19:00" \
  --category <餐饮ID> \
  --amount 100.00 \
  --currency USD \
  --actual 730.00 \
  --pay <人民币账户ID>=730.00 \
  --note 境外餐饮
```

- `--amount` 是花费币种金额（USD）。
- `--actual` 是实际结算金额（CNY）。
- 花费币种与结算账户币种不同时必须给 `--actual`。
- 不做汇率换算；统计按花费币种，余额按账户币种。

### 7.4 收入

```bash
tallytrail --db my.db add-income \
  --at "2026-10-05 09:00" \
  --category <工资分类ID> \
  --amount 20000.00 \
  --receive <银行卡ID>=20000.00
```

`--receive` 格式同 `--pay`，可重复。

### 7.5 转账

```bash
tallytrail --db my.db add-transfer \
  --at "2026-10-06 10:00" \
  --from <银行卡ID> \
  --to <支付宝ID> \
  --amount 1000.00
```

转账的两个账户必须币种相同且不能是同一账户；不改变净资产，不计入收支。

## 8. 退款与报销

### 8.1 退款

```bash
tallytrail --db my.db add-refund \
  --at "2026-10-03 10:00" \
  --ref <原支出ID> \
  --amount 20.00 \
  --receive <支付宝ID>=20.00 \
  --note 部分退款
```

### 8.2 报销

```bash
tallytrail --db my.db add-reimbursement \
  --at "2026-10-04 15:00" \
  --ref <原支出ID> \
  --amount 30.00 \
  --source <报销来源账户ID>=30.00 \
  --note 公司报销
```

- `--amount` 继承原支出的花费币种。
- 退款/报销的实际到账币种由账户决定；不同币种时需加 `--actual`。
- 单笔和累计金额都不能超过原支出。
- 原支出已作废时不能再退款/报销。

## 9. 作废

```bash
tallytrail --db my.db void <交易ID> --reason "录入错误"
```

- 作废后交易仍保留，状态变为 `VOIDED`。
- 统计、余额、预算、净资产都会排除作废交易。
- 原支出存在未作废的退款/报销时，作废会被拒绝。
- 贷款放款存在未作废的还款时，作废会被拒绝。

```text
错误: 存在未作废的关联交易，不能作废当前交易
错误: 校验失败: 交易已作废，不能重复作废
```

## 10. 查看流水与详情

### 10.1 列表

```bash
tallytrail --db my.db list
tallytrail --db my.db list --from 2026-10-01 --to 2026-10-31
tallytrail --db my.db list --type expense --type refund
tallytrail --db my.db list --category <分类ID>
tallytrail --db my.db list --account <账户ID>
tallytrail --db my.db list --tag <标签ID>
tallytrail --db my.db list --currency USD
tallytrail --db my.db list --status confirmed
tallytrail --db my.db list --limit 20 --offset 40
```

- `--type` 可重复：`expense`、`income`、`transfer`、`refund`、`reimbursement`、`loan-disburse`、`loan-repay`。
- `--tag` 可重复，表示必须同时具备所有标签。
- 不传 `--status` 时返回全部状态（含 `VOIDED`）。
- 一级分类筛选自动包含其二级分类。

### 10.2 详情

```bash
tallytrail --db my.db show <交易ID>
```

```text
交易 ID   : a60daa40-...
类型/状态 : EXPENSE / CONFIRMED
时间      : 2026-10-01 12:30
方向      : OUTFLOW
分类      : 午餐
花费金额  : 45.50 CNY
实际金额  : 45.50
备注      : 工作日午餐
结算行:
账户    账户 ID         角色     金额   币种
------  ---------------  -------  -----  ----
支付宝  6796b27f-...     PAYMENT  45.50  CNY
```

## 11. 余额、汇总与趋势

### 11.1 账户余额

```bash
tallytrail --db my.db balance
tallytrail --db my.db balance --as-of "2026-10-31 23:59"
tallytrail --db my.db balance --as-of 2026-10-31
```

### 11.2 收支汇总

```bash
tallytrail --db my.db summary --from 2026-10-01 --to 2026-10-31
```

```text
币种  收入  支出(毛)  退款   报销  净支出
----  ----  --------  -----  ----  ------
CNY   0.00  45.50     20.00  0.00  25.50
USD   0.00  100.00    0.00   0.00  100.00
```

- 按花费币种分组。
- 净支出 = 毛支出 − 退款 − 报销。
- 贷款利息计入支出；本金、放款、转账不计入。

### 11.3 分类支出时间序列

```bash
tallytrail --db my.db spending \
  --category <一级分类ID> \
  --from 2026-01-01 \
  --to 2026-12-31 \
  --granularity monthly
```

粒度：`daily`、`weekly`、`monthly`（默认 monthly）。

```text
周期（左闭右开）         币种  支出净额
-----------------------  ----  --------
2026-10-01 ~ 2026-11-01  CNY   25.50
2026-10-01 ~ 2026-11-01  USD   100.00
```

一级分类自动包含二级分类；退款/报销冲减支出；贷款利息按分类计入。

### 11.4 净资产

```bash
tallytrail --db my.db networth --from 2026-10-01 --to 2026-10-31
```

```text
周期起点    币种  总资产   贷款未还本金  净资产
----------  ----  -------  ------------  -------
2026-10-01  CNY   9744.50  5000.00       4744.50
```

- 净资产 = 账户余额合计 − 贷款未还本金。
- 按币种分组，不做汇率折算。
- 未来起始的贷款不计入历史时点。
- 转账不改变净资产。

## 12. 贷款

### 12.1 新建贷款

```bash
tallytrail --db my.db add-loan \
  --lender 某银行 \
  --principal 5000.00 \
  --rate-bp 450 \
  --start "2026-10-07" \
  --term-months 12 \
  --account <银行卡ID>
```

- `--rate-bp` 是年化基点：`450` = 4.50%。
- `--start` 省略时使用当前时间。
- `--account` 可选，币种必须与贷款币种一致。

### 12.2 放款

```bash
tallytrail --db my.db add-disbursement \
  --at "2026-10-07 10:00" \
  --loan <贷款ID> \
  --amount 5000.00 \
  --to <银行卡ID>
```

放款计入账户资产，但不计入收入。

### 12.3 还款

```bash
tallytrail --db my.db add-repayment \
  --at "2026-11-07 10:00" \
  --loan <贷款ID> \
  --amount 1030.00 \
  --principal 1000.00 \
  --interest 30.00 \
  --from <银行卡ID> \
  --category <利息分类ID>
```

- `--principal + --interest` 必须等于 `--amount`。
- 本金不能超过剩余未还本金。
- `--category` 可省略；填写时必须是 `expense` 分类。
- 利息计入收支汇总与分类支出序列，但贷款交易不计入预算。

### 12.4 查询

```bash
tallytrail --db my.db loan list
tallytrail --db my.db loan show <贷款ID>
```

```text
ID          贷款方  本金         年化   起始        期限(月)  未还本金  状态
----------  ------  -----------  -----  ----------  --------  --------  ------
57287d52...  某银行  5000.00 CNY  4.50%  2026-10-07  12        5000.00   ACTIVE
```

```text
贷款 ID     : 57287d52-...
贷款方      : 某银行
本金/已还   : 5000.00 / 0.00 CNY
未还本金    : 5000.00
状态        : ACTIVE
```

## 13. 预算

### 13.1 新建

```bash
# 分类总预算（一级分类，包含二级）
tallytrail --db my.db budget add \
  --name 十月餐饮 \
  --scope category-l1 \
  --category <餐饮分类ID> \
  --period monthly \
  --amount 300.00

# 二级分类预算
tallytrail --db my.db budget add \
  --name 十月外卖 \
  --scope category-l2 \
  --category <外卖分类ID> \
  --period monthly \
  --amount 100.00

# 总预算（不指定分类）
tallytrail --db my.db budget add \
  --name 十月总预算 \
  --scope total \
  --period monthly \
  --amount 3000.00

# 自定义周期
tallytrail --db my.db budget add \
  --name 国庆预算 \
  --scope total \
  --period custom \
  --start "2026-10-01 00:00" \
  --end "2026-10-08 00:00" \
  --amount 1000.00
```

- `--scope`：`total`、`category-l1`、`category-l2`。
- `--period`：`monthly`、`yearly`、`custom`。
- `custom` 必须同时给 `--start`、`--end`，且开始早于结束。
- `--currency` 默认 CNY；预算只统计同币种交易。

### 13.2 查看

```bash
tallytrail --db my.db budget list
tallytrail --db my.db budget list --as-of "2026-10-20"
tallytrail --db my.db budget list --all      # 只显示预算定义，含停用

tallytrail --db my.db budget show <预算ID>
```

```text
ID          名称      Scope        分类     预算        已用   剩余    周期
----------  --------  -----------  -------  ----------  -----  ------  -----------------------
b5a4a88a...  十月餐饮  CATEGORY_L1  ...      300.00 CNY  25.50  274.50  2026-10-01 ~ 2026-11-01
```

```text
预算 ID   : b5a4a88a-...
名称      : 十月餐饮
Scope     : CATEGORY_L1
分类      : 9bc8202f-...
周期      : 2026-10-01 ~ 2026-11-01（左闭右开，UTC 周期）
预算/已用 : 300.00 / 25.50 CNY
剩余      : 274.50
```

### 13.3 管理

```bash
tallytrail --db my.db budget disable <预算ID>
tallytrail --db my.db budget enable <预算ID>
tallytrail --db my.db budget delete <预算ID>
```

停用预算不会删除历史数据，但不会出现在默认使用情况列表中。

## 14. 数据库维护

### 14.1 checkpoint

```bash
tallytrail --db my.db checkpoint
```

数据库使用 WAL。**直接复制 `.db` 文件前**请先执行 checkpoint，或确保程序已经正常关闭；否则最近的写入可能还在 `my.db-wal` 中。

安全备份：

```bash
tallytrail --db my.db checkpoint
cp my.db my-backup.db
```

CLI 每条命令结束都会自动 checkpoint，因此正常使用后立刻复制通常也是安全的；显式执行更稳妥。

## 15. 完整链路示例

```bash
DB=my.db

# 账户
tallytrail --db $DB account add --name 招商银行 --type bank --initial 10000.00
tallytrail --db $DB account add --name 支付宝 --type e-wallet --initial 500.00

# 分类
tallytrail --db $DB category add --name 餐饮 --kind expense
tallytrail --db $DB category add --name 午餐 --kind expense --parent <餐饮ID>
tallytrail --db $DB category add --name 工资 --kind income

# 组合支付
tallytrail --db $DB add-expense --at "2026-10-01 12:00" \
  --category <午餐ID> --amount 100.00 \
  --pay <银行卡ID>=60.00 --pay <支付宝ID>=40.00

# 境外消费
tallytrail --db $DB add-expense --at "2026-10-02 19:00" \
  --category <餐饮ID> --amount 100.00 --currency USD \
  --actual 730.00 --pay <银行卡ID>=730.00

# 两次退款
tallytrail --db $DB add-refund --at "2026-10-03 10:00" \
  --ref <原支出ID> --amount 20.00 --receive <支付宝ID>=20.00
tallytrail --db $DB add-refund --at "2026-10-04 10:00" \
  --ref <原支出ID> --amount 10.00 --receive <支付宝ID>=10.00

# 报销
tallytrail --db $DB add-reimbursement --at "2026-10-05 10:00" \
  --ref <境外消费ID> --amount 30.00 --source <报销账户ID>=30.00

# 贷款与两次还款
tallytrail --db $DB add-loan --lender 某银行 --principal 5000.00 \
  --rate-bp 450 --start "2026-10-07" --account <银行卡ID>
tallytrail --db $DB add-disbursement --at "2026-10-07 10:00" \
  --loan <贷款ID> --amount 5000.00 --to <银行卡ID>
tallytrail --db $DB add-repayment --at "2026-11-07 10:00" \
  --loan <贷款ID> --amount 1030.00 --principal 1000.00 --interest 30.00 \
  --from <银行卡ID>
tallytrail --db $DB add-repayment --at "2026-12-07 10:00" \
  --loan <贷款ID> --amount 1015.00 --principal 1000.00 --interest 15.00 \
  --from <银行卡ID>

# 作废一条退款
tallytrail --db $DB void <退款ID> --reason "退款录错"

# 再尝试作废原消费：应被拒绝
tallytrail --db $DB void <原支出ID> --reason "测试"
# 错误: 存在未作废的关联交易，不能作废当前交易

# 统计
tallytrail --db $DB balance
tallytrail --db $DB summary --from 2026-10-01 --to 2026-12-31
tallytrail --db $DB networth --from 2026-10-01 --to 2026-12-31
tallytrail --db $DB budget list --as-of 2026-12-20
```

## 16. 常见错误与排查

| 提示 | 原因 | 处理 |
| --- | --- | --- |
| `记录不存在` | ID 写错，或账户/分类/标签已删除 | 用 `list` 子命令重新获取 ID |
| `校验失败: 金额必须大于 0` | 传了 0 或负数 | 金额必须为正 |
| `校验失败: 金额最多两位小数` | 输入 `1.234` | 最多两位小数 |
| `校验失败: 时间格式必须为 ...` | 日期格式不对 | 使用 `YYYY-MM-DD HH:MM` 或 `YYYY-MM-DD` |
| `校验失败: 至少需要一条结算行` | 支出/收入没给 `--pay`/`--receive` | 至少给一个结算账户 |
| `同一笔交易的结算账户币种必须一致` | 混用了 CNY 和 USD 账户 | 拆成两笔或统一币种 |
| `校验失败: 花费币种与结算账户币种不同，必须显式填写 actual_amount_cents` | 境外消费没给 `--actual` | 补 `--actual` |
| `校验失败: 转出账户与转入账户不能相同` | 转账两端相同 | 选择不同账户 |
| `分类已被引用或仍有子分类，不能执行该操作` | 删除/改 kind 的分类仍被使用 | 先停用，或迁移引用 |
| `存在未作废的关联交易，不能作废当前交易` | 原支出仍有退款/报销，或放款仍有还款 | 先作废关联交易 |
| `校验失败: 该贷款已结清，不能继续还款` | 本金已还完 | 不再登记还款 |
| `校验失败: 还款本金部分不能超过剩余未还本金` | 本金超额 | 调整本金/利息拆分 |
| `校验失败: TOTAL 预算不能指定分类` | `--scope total` 同时给了 `--category` | 去掉 `--category` |
| `校验失败: CUSTOM 预算必须填写 start_at` | 自定义周期缺日期 | 补 `--start` 和 `--end` |
| `校验失败: 预算 scope 与分类层级不匹配` | 一级预算用了二级分类，反之亦然 | 检查分类层级 |

CLI 失败时向 stderr 输出 `错误: ...` 并以退出码 `1` 结束，脚本可据此判断。

## 17. 帮助

每个命令都有内建帮助：

```bash
tallytrail --help
tallytrail add-expense --help
tallytrail budget --help
tallytrail budget add --help
```

建议先看 `--help` 确认当前版本的参数名称。
