mod commands;
mod util;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use tallytrail_core::CoreHandle;

use util::CliResult;

#[derive(Debug, Parser)]
#[command(
    name = "tallytrail",
    version,
    about = "TallyTrail —— 本地单文件记账与资产管理 CLI",
    long_about = "所有数据保存在一个 SQLite 文件中。日期输入格式为 `YYYY-MM-DD HH:MM`（也支持仅日期 `YYYY-MM-DD`）。金额使用十进制，最多两位小数。"
)]
struct Cli {
    /// SQLite 单文件数据库路径
    #[arg(long, global = true, default_value = "tallytrail.db")]
    db: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 账户管理
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// 分类管理（最多两级）
    Category {
        #[command(subcommand)]
        command: CategoryCommand,
    },
    /// 标签管理
    Tag {
        #[command(subcommand)]
        command: TagCommand,
    },
    /// 记一笔支出（支持组合支付与境外消费双金额）
    AddExpense(AddExpenseArgs),
    /// 记一笔收入
    AddIncome(AddIncomeArgs),
    /// 账户间转账
    AddTransfer(AddTransferArgs),
    /// 对一笔支出登记退款
    AddRefund(AddRefundArgs),
    /// 对一笔支出登记报销
    AddReimbursement(AddReimbursementArgs),
    /// 新建贷款
    AddLoan(AddLoanArgs),
    /// 贷款放款
    AddDisbursement(AddDisbursementArgs),
    /// 贷款还款（本金/利息拆分）
    AddRepayment(AddRepaymentArgs),
    /// 作废交易
    Void {
        /// 交易 ID
        id: String,
        /// 作废原因
        #[arg(long)]
        reason: String,
    },
    /// 查看流水列表
    List(ListArgs),
    /// 查看单笔交易详情
    Show {
        /// 交易 ID
        id: String,
    },
    /// 账户余额（按账户与币种）
    Balance {
        /// 截止时间，默认当前
        #[arg(long)]
        as_of: Option<String>,
    },
    /// 收支汇总（花费币种口径）
    Summary {
        /// 起始时间（含）
        #[arg(long)]
        from: String,
        /// 结束时间（含；仅日期时包含整日）
        #[arg(long)]
        to: String,
    },
    /// 分类支出时间序列
    Spending {
        /// 分类 ID（一级含所有二级）
        #[arg(long)]
        category: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        /// 时间粒度
        #[arg(long, value_enum, default_value_t = GranularityArg::Monthly)]
        granularity: GranularityArg,
    },
    /// 净资产时间序列（资产 − 贷款未还本金，按币种分组）
    Networth {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, value_enum, default_value_t = GranularityArg::Monthly)]
        granularity: GranularityArg,
    },
    /// 预算管理
    Budget {
        #[command(subcommand)]
        command: BudgetCommand,
    },
    /// 贷款查询
    Loan {
        #[command(subcommand)]
        command: LoanCommand,
    },
    /// 手动执行 WAL checkpoint（拷贝数据库前建议执行）
    Checkpoint,
}

#[derive(Debug, Subcommand)]
enum AccountCommand {
    /// 新建账户
    Add {
        #[arg(long)]
        name: String,
        #[arg(long = "type", value_enum, default_value_t = AccountTypeArg::Bank)]
        account_type: AccountTypeArg,
        #[arg(long)]
        currency: Option<String>,
        /// 初始余额（分币种；信用卡欠款可填负数）
        #[arg(long, default_value = "0")]
        initial: String,
    },
    /// 列出账户
    List {
        #[arg(long)]
        all: bool,
    },
    /// 重命名账户
    Rename { id: String, name: String },
    /// 启用账户
    Enable { id: String },
    /// 停用账户
    Disable { id: String },
}

#[derive(Debug, Subcommand)]
enum CategoryCommand {
    /// 新建分类
    Add {
        #[arg(long)]
        name: String,
        #[arg(long, value_enum)]
        kind: KindArg,
        /// 父分类 ID；不填则为一级分类
        #[arg(long)]
        parent: Option<String>,
        #[arg(long, default_value_t = 0)]
        sort: i32,
    },
    /// 列出分类
    List {
        #[arg(long, value_enum)]
        kind: Option<KindArg>,
        #[arg(long)]
        all: bool,
    },
    /// 重命名分类
    Rename { id: String, name: String },
    /// 启用分类
    Enable { id: String },
    /// 停用分类
    Disable { id: String },
    /// 物理删除（无子分类且无交易/预算引用时）
    Delete { id: String },
    /// 修改分类 kind（有引用时会被拒绝）
    SetKind {
        id: String,
        #[arg(long, value_enum)]
        kind: KindArg,
    },
}

#[derive(Debug, Subcommand)]
enum TagCommand {
    Add { name: String },
    List,
    Rename { id: String, name: String },
    Delete { id: String },
}

#[derive(Debug, Args)]
struct AddExpenseArgs {
    /// 发生时间 YYYY-MM-DD HH:MM
    #[arg(long)]
    at: String,
    #[arg(long)]
    category: String,
    /// 花费金额（商品原价）
    #[arg(long)]
    amount: String,
    /// 花费币种，默认 CNY
    #[arg(long)]
    currency: Option<String>,
    /// 实际结算金额；与结算账户币种不同时必填
    #[arg(long)]
    actual: Option<String>,
    /// 结算账户与金额，格式 account_id=12.34，可重复
    #[arg(long = "pay", value_name = "ACCOUNT=AMOUNT", num_args = 1..)]
    pay: Vec<String>,
    #[arg(long)]
    note: Option<String>,
    /// 标签 ID，可重复
    #[arg(long = "tag")]
    tags: Vec<String>,
}

#[derive(Debug, Args)]
struct AddIncomeArgs {
    #[arg(long)]
    at: String,
    #[arg(long)]
    category: String,
    #[arg(long)]
    amount: String,
    #[arg(long)]
    currency: Option<String>,
    #[arg(long)]
    actual: Option<String>,
    /// 收款账户与金额，格式 account_id=12.34，可重复
    #[arg(long = "receive", value_name = "ACCOUNT=AMOUNT", num_args = 1..)]
    receive: Vec<String>,
    #[arg(long)]
    note: Option<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
}

#[derive(Debug, Args)]
struct AddTransferArgs {
    #[arg(long)]
    at: String,
    #[arg(long)]
    from: String,
    #[arg(long)]
    to: String,
    #[arg(long)]
    amount: String,
    #[arg(long)]
    note: Option<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
}

#[derive(Debug, Args)]
struct AddRefundArgs {
    #[arg(long)]
    at: String,
    /// 原支出交易 ID
    #[arg(long = "ref")]
    reference: String,
    #[arg(long)]
    amount: String,
    #[arg(long)]
    actual: Option<String>,
    /// 退款入账账户与金额，格式 account_id=12.34，可重复
    #[arg(long = "receive", value_name = "ACCOUNT=AMOUNT", num_args = 1..)]
    receive: Vec<String>,
    #[arg(long)]
    note: Option<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
}

#[derive(Debug, Args)]
struct AddReimbursementArgs {
    #[arg(long)]
    at: String,
    #[arg(long = "ref")]
    reference: String,
    #[arg(long)]
    amount: String,
    #[arg(long)]
    actual: Option<String>,
    /// 报销来源账户与金额，格式 account_id=12.34，可重复
    #[arg(long = "source", value_name = "ACCOUNT=AMOUNT", num_args = 1..)]
    source: Vec<String>,
    #[arg(long)]
    note: Option<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
}

#[derive(Debug, Args)]
struct AddLoanArgs {
    #[arg(long)]
    lender: String,
    #[arg(long)]
    principal: String,
    #[arg(long)]
    currency: Option<String>,
    /// 年化基点，4.5% = 450
    #[arg(long, default_value_t = 0)]
    rate_bp: i32,
    /// 起始时间，默认当前
    #[arg(long)]
    start: Option<String>,
    #[arg(long)]
    term_months: Option<i32>,
    #[arg(long)]
    account: Option<String>,
    #[arg(long)]
    note: Option<String>,
}

#[derive(Debug, Args)]
struct AddDisbursementArgs {
    #[arg(long)]
    at: String,
    #[arg(long)]
    loan: String,
    #[arg(long)]
    amount: String,
    /// 收款账户
    #[arg(long)]
    to: String,
    #[arg(long)]
    note: Option<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
}

#[derive(Debug, Args)]
struct AddRepaymentArgs {
    #[arg(long)]
    at: String,
    #[arg(long)]
    loan: String,
    /// 实际还款总额
    #[arg(long)]
    amount: String,
    /// 其中本金
    #[arg(long)]
    principal: String,
    /// 其中利息
    #[arg(long, default_value = "0")]
    interest: String,
    /// 还款账户
    #[arg(long)]
    from: String,
    /// 利息归属的支出分类，可空
    #[arg(long)]
    category: Option<String>,
    #[arg(long)]
    note: Option<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
}

#[derive(Debug, Args)]
struct ListArgs {
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    to: Option<String>,
    /// 交易类型，可重复
    #[arg(long = "type", value_enum)]
    types: Vec<TxnTypeArg>,
    #[arg(long)]
    category: Option<String>,
    #[arg(long)]
    account: Option<String>,
    /// 标签 ID，可重复（需同时具备）
    #[arg(long = "tag")]
    tags: Vec<String>,
    #[arg(long)]
    currency: Option<String>,
    #[arg(long, value_enum)]
    status: Option<StatusArg>,
    #[arg(long)]
    limit: Option<u32>,
    #[arg(long)]
    offset: Option<u32>,
}

#[derive(Debug, Subcommand)]
enum BudgetCommand {
    /// 新建预算
    Add {
        #[arg(long)]
        name: String,
        #[arg(long, value_enum)]
        scope: ScopeArg,
        #[arg(long)]
        category: Option<String>,
        #[arg(long, value_enum)]
        period: PeriodArg,
        #[arg(long)]
        amount: String,
        #[arg(long)]
        currency: Option<String>,
        #[arg(long)]
        start: Option<String>,
        #[arg(long)]
        end: Option<String>,
    },
    /// 列出预算使用情况（默认当前周期）
    List {
        #[arg(long)]
        as_of: Option<String>,
        /// 显示预算定义（含停用），不计算使用额
        #[arg(long)]
        all: bool,
    },
    /// 查看单个预算使用情况
    Show {
        id: String,
        #[arg(long)]
        as_of: Option<String>,
    },
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    Delete {
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum LoanCommand {
    List,
    Show { id: String },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AccountTypeArg {
    Bank,
    Cash,
    EWallet,
    Credit,
    ReimburseSource,
    Other,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum KindArg {
    Expense,
    Income,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TxnTypeArg {
    Expense,
    Income,
    Transfer,
    Refund,
    Reimbursement,
    LoanDisburse,
    LoanRepay,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StatusArg {
    Confirmed,
    Voided,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum GranularityArg {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ScopeArg {
    Total,
    CategoryL1,
    CategoryL2,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PeriodArg {
    Monthly,
    Yearly,
    Custom,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("错误: {error}");
        std::process::exit(1);
    }
}

fn run() -> CliResult<()> {
    let cli = Cli::parse();
    let core = CoreHandle::open(&cli.db)?;
    commands::dispatch(&core, cli.command)?;
    core.checkpoint()?;
    Ok(())
}
