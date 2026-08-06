//! 模型用量监控
//!
//! 按本地自然日聚合各应用/模型的 token 与成本，供托盘「今日用量」菜单展示。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::Connection;

/// 国产供应商（CNY 计价）美元折算汇率，与 model_pricing 种子口径一致。
pub const CNY_PER_USD: f64 = 7.14;

/// 当日单个模型用量行（托盘展示）
#[derive(Debug, Clone)]
pub struct TodayUsageRow {
    /// 应用类型（claude-desktop 已折叠为 claude）
    pub app_type: String,
    /// 有效计价模型（pricing_model 优先，空值回落 model）
    pub model: String,
    /// input + output + cache_read + cache_creation 总 token 数
    pub tokens: i64,
    /// 成本（USD）
    pub cost_usd: f64,
    /// 供应商是否为国内计价（category = 'cn_official'），用于 ¥/$ 展示
    pub is_cn_provider: bool,
}

impl Database {
    /// 查询当日（本地时区）按应用 + 模型聚合的用量，按成本降序。
    pub fn get_today_usage_by_model(&self) -> Result<Vec<TodayUsageRow>, AppError> {
        let conn = lock_conn!(self.conn);
        Self::get_today_usage_by_model_conn(&conn)
    }

    pub(crate) fn get_today_usage_by_model_conn(
        conn: &Connection,
    ) -> Result<Vec<TodayUsageRow>, AppError> {
        // is_cn 判定：provider 可 join 时以 category='cn_official' 为准；
        // session 行（provider_id 为占位符，join 不到）按模型族前缀兜底，
        // 与 model_pricing 种子里的国内模型口径一致。
        let mut stmt = conn.prepare(
            "SELECT
                CASE WHEN l.app_type = 'claude-desktop' THEN 'claude' ELSE l.app_type END AS app_type,
                COALESCE(NULLIF(l.pricing_model, ''), l.model) AS model,
                SUM(l.input_tokens + l.output_tokens + l.cache_read_tokens + l.cache_creation_tokens) AS tokens,
                SUM(CAST(l.total_cost_usd AS REAL)) AS cost_usd,
                MAX(
                    CASE
                        WHEN p.category = 'cn_official' THEN 1
                        WHEN COALESCE(NULLIF(l.pricing_model, ''), l.model) LIKE 'deepseek-%' THEN 1
                        WHEN COALESCE(NULLIF(l.pricing_model, ''), l.model) LIKE 'qwen%' THEN 1
                        WHEN COALESCE(NULLIF(l.pricing_model, ''), l.model) LIKE 'glm-%' THEN 1
                        WHEN COALESCE(NULLIF(l.pricing_model, ''), l.model) LIKE 'kimi%' THEN 1
                        WHEN COALESCE(NULLIF(l.pricing_model, ''), l.model) LIKE 'minimax%' THEN 1
                        WHEN COALESCE(NULLIF(l.pricing_model, ''), l.model) LIKE 'doubao%' THEN 1
                        WHEN COALESCE(NULLIF(l.pricing_model, ''), l.model) LIKE 'hunyuan%' THEN 1
                        WHEN COALESCE(NULLIF(l.pricing_model, ''), l.model) LIKE 'moonshot%' THEN 1
                        WHEN COALESCE(NULLIF(l.pricing_model, ''), l.model) LIKE 'step-%' THEN 1
                        WHEN COALESCE(NULLIF(l.pricing_model, ''), l.model) LIKE 'ernie%' THEN 1
                        ELSE 0
                    END
                ) AS is_cn
             FROM proxy_request_logs l
             LEFT JOIN providers p
                    ON l.provider_id = p.id AND l.app_type = p.app_type
             WHERE date(datetime(l.created_at, 'unixepoch', 'localtime')) = date('now', 'localtime')
             GROUP BY 1, 2
             ORDER BY cost_usd DESC, tokens DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TodayUsageRow {
                app_type: row.get(0)?,
                model: row.get(1)?,
                tokens: row.get(2)?,
                cost_usd: row.get(3)?,
                is_cn_provider: row.get::<_, i64>(4)? != 0,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 查询当日（本地时区）按应用分组、组内按模型聚合的用量。
    /// 复用按（应用，模型）聚合的结果在 Rust 侧分组：应用按总成本降序，组内行按成本降序。
    pub fn get_today_usage_by_app_grouped(&self) -> Result<Vec<TodayUsageGroup>, AppError> {
        let conn = lock_conn!(self.conn);
        Self::get_today_usage_by_app_grouped_conn(&conn)
    }

    pub(crate) fn get_today_usage_by_app_grouped_conn(
        conn: &Connection,
    ) -> Result<Vec<TodayUsageGroup>, AppError> {
        let rows = Self::get_today_usage_by_model_conn(conn)?;
        let mut groups: Vec<TodayUsageGroup> = Vec::new();
        for row in rows {
            match groups.iter_mut().find(|g| g.app_type == row.app_type) {
                Some(g) => {
                    g.total_tokens += row.tokens;
                    g.total_cost_usd += row.cost_usd;
                    g.rows.push(row);
                }
                None => groups.push(TodayUsageGroup {
                    app_type: row.app_type.clone(),
                    total_tokens: row.tokens,
                    total_cost_usd: row.cost_usd,
                    rows: vec![row],
                }),
            }
        }
        // 底层查询按 cost_usd DESC 排序，分组内行序已满足；组间按总成本降序补齐
        groups.sort_by(|a, b| {
            b.total_cost_usd
                .partial_cmp(&a.total_cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(groups)
    }
}

/// 当日按应用分组的用量（托盘「今日用量」子菜单展示）
#[derive(Debug, Clone)]
pub struct TodayUsageGroup {
    /// 应用类型（与 TodayUsageRow.app_type 一致）
    pub app_type: String,
    /// 该应用当日总 token 数
    pub total_tokens: i64,
    /// 该应用当日总成本（USD）
    pub total_cost_usd: f64,
    /// 组内模型行，按成本降序
    pub rows: Vec<TodayUsageRow>,
}

/// token 数格式化：≥1 亿 → 亿；≥1 万 → w（万），整数去尾；其余原样。
pub fn format_tokens(tokens: i64) -> String {
    if tokens >= 100_000_000 {
        let yi = tokens as f64 / 100_000_000.0;
        format!("{yi:.1}亿")
    } else if tokens >= 10_000 {
        let w = tokens as f64 / 10_000.0;
        let rounded = w.round() as i64;
        if (w - rounded as f64).abs() < 0.05 {
            format!("{rounded}w")
        } else {
            format!("{w:.1}w")
        }
    } else {
        tokens.to_string()
    }
}

/// 成本格式化：国内供应商按 CNY 展示（¥），其余 USD（$）。
/// 金额符号放在数字之后，与「今日用量」托盘行格式一致。
pub fn format_cost(cost_usd: f64, is_cn_provider: bool) -> String {
    let value = if is_cn_provider {
        cost_usd * CNY_PER_USD
    } else {
        cost_usd
    };
    let symbol = if is_cn_provider { "¥" } else { "$" };
    if value >= 0.01 {
        format!("{value:.2}{symbol}")
    } else {
        format!("{value:.4}{symbol}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_formats_wan_and_yi() {        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(9_999), "9999");
        assert_eq!(format_tokens(10_000), "1w");
        assert_eq!(format_tokens(50_000), "5w");
        assert_eq!(format_tokens(500_000), "50w");
        assert_eq!(format_tokens(125_000), "12.5w");
        assert_eq!(format_tokens(1_000_000), "100w");
        assert_eq!(format_tokens(120_000_000), "1.2亿");
    }

    #[test]
    fn cost_formats_usd_and_cny() {
        assert_eq!(format_cost(10.0, false), "10.00$");
        assert_eq!(format_cost(1.0, false), "1.00$");
        assert_eq!(format_cost(0.005, false), "0.0050$");
        assert_eq!(format_cost(1.4, true), "10.00¥");
        assert_eq!(format_cost(0.0005, true), "0.0036¥");
    }

    #[test]
    fn live_db_today_usage_query() {
        let home = std::env::var("HOME").expect("HOME");
        let db_path = std::path::Path::new(&home).join(".cc-switch/cc-switch.db");
        if !db_path.exists() {
            eprintln!("skip: no db at {db_path:?}");
            return;
        }
        let conn = Connection::open(&db_path).expect("open live db");
        let rows = Database::get_today_usage_by_model_conn(&conn).expect("today usage query");
        eprintln!("live rows: {rows:?}");
    }
}
