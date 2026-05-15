use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use super::AppState;

#[derive(Debug, Serialize)]
pub struct FinanceServiceHealth {
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FinanceHealthSnapshot {
    pub intel: FinanceServiceHealth,
    pub risk: FinanceServiceHealth,
    pub exec: FinanceServiceHealth,
    pub checked_at: String,
}

#[derive(Debug, Serialize)]
pub struct FinanceOverviewSnapshot {
    pub health: FinanceHealthSnapshot,
    pub market_status: Value,
    pub balance: Value,
    pub positions: Vec<Value>,
    pub watchlist: Vec<Value>,
    pub pnl: Value,
    pub active_order_count: usize,
    pub today_trade_count: usize,
}

#[derive(Debug, Serialize)]
pub struct FinanceOrdersSnapshot {
    pub balance: Value,
    pub positions: Vec<Value>,
    pub orders: Vec<Value>,
    pub trades: Vec<Value>,
    pub pnl: Value,
}

#[derive(Debug, Serialize)]
pub struct FinanceSymbolSnapshot {
    pub symbol: String,
    pub news: Vec<Value>,
    pub tables: Vec<Value>,
}

#[derive(Debug, Serialize)]
pub struct FinanceOrderPreview {
    pub symbol: String,
    pub market: String,
    pub intent_direction: String,
    pub order_direction: String,
    pub approval_status: String,
    pub price: f64,
    pub quantity: i64,
    pub amount: f64,
    pub position_pct: f64,
    pub order_type: String,
    pub entry_rule: Option<String>,
    pub thesis: Option<String>,
    pub notes: Vec<String>,
}

fn now_iso() -> String {
    chrono::Local::now().to_rfc3339()
}

fn parse_tool_payload(value: Value) -> Result<Value, String> {
    match value {
        Value::String(text) => serde_json::from_str::<Value>(&text).map_err(|err| {
            format!("MCP 返回不是合法 JSON: {err}. 原始返回: {text}")
        }),
        other => Ok(other),
    }
}

async fn finance_call(
    state: &AppState,
    server: &str,
    tool: &str,
    params: Value,
) -> Result<Value, String> {
    let raw = state
        .app
        .mcp_orchestrator
        .call_tool(server, tool, &params)
        .await
        .map_err(|err| err.to_string())?;
    parse_tool_payload(raw)
}

async fn server_configured(state: &AppState, name: &str) -> bool {
    state
        .app
        .list_mcp_servers()
        .await
        .into_iter()
        .any(|server| server.name == name && server.enabled)
}

async fn probe_health(state: &AppState, server: &str, tool: &str, params: Value) -> FinanceServiceHealth {
    if !server_configured(state, server).await {
        return FinanceServiceHealth {
            status: "unconfigured".into(),
            message: Some("未配置或未启用".into()),
        };
    }
    match finance_call(state, server, tool, params).await {
        Ok(_) => FinanceServiceHealth {
            status: "up".into(),
            message: None,
        },
        Err(error) => FinanceServiceHealth {
            status: "down".into(),
            message: Some(error),
        },
    }
}

async fn build_health_snapshot(app_state: &AppState) -> FinanceHealthSnapshot {
    let intel = probe_health(app_state, "intel", "is_trading_day", json!({})).await;
    let risk = probe_health(app_state, "risk", "daily_pnl", json!({})).await;
    let exec = probe_health(app_state, "exec", "get_balance", json!({})).await;
    FinanceHealthSnapshot {
        intel,
        risk,
        exec,
        checked_at: now_iso(),
    }
}

fn value_array(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(ToString::to_string)
}

fn value_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn parse_first_number(text: &str) -> Option<f64> {
    let mut buf = String::new();
    let mut seen_digit = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            buf.push(ch);
            seen_digit = true;
        } else if seen_digit {
            break;
        }
    }
    if buf.is_empty() {
        None
    } else {
        buf.parse::<f64>().ok()
    }
}

fn infer_price_from_tables(payload: &Value) -> Option<f64> {
    let tables = value_array(payload, "tables");
    for table in tables {
        let columns = value_array(&table, "columns");
        let rows = value_array(&table, "rows");
        if rows.is_empty() {
            continue;
        }
        let Some(first_row) = rows.first() else {
            continue;
        };
        let preferred_keys = ["最新价", "close", "收盘", "price", "现价"];
        for key in preferred_keys {
            if let Some(price) = first_row.get(key).and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(parse_first_number))
            }) {
                return Some(price);
            }
        }
        for column in columns {
            let Some(column_name) = column.as_str() else { continue };
            if let Some(price) = first_row.get(column_name).and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(parse_first_number))
            }) {
                return Some(price);
            }
        }
    }
    None
}

async fn infer_latest_price(state: &AppState, symbol: &str) -> Result<f64, String> {
    let watchlist = finance_call(state, "intel", "get_watchlist", json!({})).await?;
    if let Some(price) = value_array(&watchlist, "stocks")
        .into_iter()
        .find(|item| value_string(item, "code").as_deref() == Some(symbol))
        .and_then(|item| value_f64(&item, "price"))
    {
        return Ok(price);
    }

    let tables = finance_call(
        state,
        "intel",
        "query_data",
        json!({
            "query": format!("{symbol} 最新价"),
            "symbol": symbol,
            "metrics": ["close"]
        }),
    )
    .await?;

    infer_price_from_tables(&tables).ok_or_else(|| format!("无法为 {symbol} 推断最新价格"))
}

async fn build_order_preview(state: &AppState, intent: &Value) -> Result<FinanceOrderPreview, String> {
    let symbol = value_string(intent, "symbol").ok_or_else(|| "intent 缺少 symbol".to_string())?;
    let market = value_string(intent, "market").unwrap_or_else(|| "SSE".into());
    let intent_direction = value_string(intent, "direction").unwrap_or_else(|| "buy".into());
    let approval_status = value_string(intent, "approval_status").unwrap_or_else(|| "pending".into());
    if approval_status != "approved" && approval_status != "reduced" {
        return Err("只有 approved 或 reduced 的意图才可进入执行准备".into());
    }

    let price = value_string(intent, "entry_rule")
        .as_deref()
        .and_then(parse_first_number)
        .or_else(|| value_f64(intent, "price"))
        .unwrap_or(0.0);
    let resolved_price = if price > 0.0 { price } else { infer_latest_price(state, &symbol).await? };

    let balance = finance_call(state, "exec", "get_balance", json!({})).await?;
    let total_assets = value_f64(&balance, "total_assets").ok_or_else(|| "无法读取 total_assets".to_string())?;
    let positions_payload = finance_call(state, "exec", "get_positions", json!({})).await?;
    let positions = value_array(&positions_payload, "positions");
    let current_position = positions
        .iter()
        .find(|item| value_string(item, "symbol").as_deref() == Some(symbol.as_str()));

    let position_pct = value_f64(intent, "adjusted_position_pct")
        .or_else(|| value_f64(intent, "max_position_pct"))
        .unwrap_or(0.1);

    let order_direction = match intent_direction.as_str() {
        "buy" => "buy",
        "sell" | "reduce" => "sell",
        other => return Err(format!("暂不支持的 direction: {other}")),
    }
    .to_string();

    let quantity = match intent_direction.as_str() {
        "buy" => {
            let target_amount = total_assets * position_pct.max(0.0);
            ((target_amount / resolved_price) / 100.0).floor() as i64 * 100
        }
        "sell" => current_position
            .and_then(|item| item.get("available_quantity").and_then(Value::as_i64))
            .unwrap_or(0),
        "reduce" => {
            let available = current_position
                .and_then(|item| item.get("available_quantity").and_then(Value::as_i64))
                .unwrap_or(0);
            ((available as f64 * position_pct.clamp(0.0, 1.0)) / 100.0).floor() as i64 * 100
        }
        _ => 0,
    };

    if quantity <= 0 {
        return Err("根据当前价格和仓位规则算出的数量为 0，无法下单".into());
    }

    let mut notes = Vec::new();
    notes.push(format!("价格基于 {}", value_string(intent, "entry_rule").filter(|s| parse_first_number(s).is_some()).map(|_| "entry_rule").unwrap_or_else(|| "实时行情".into())));
    if approval_status == "reduced" {
        notes.push("本意图已被风控降仓，数量按 adjusted_position_pct 计算".into());
    }
    if intent_direction == "sell" {
        notes.push("卖出默认使用当前可卖数量".into());
    }
    if intent_direction == "reduce" {
        notes.push("减仓默认按可卖数量乘以建议仓位比例估算".into());
    }

    Ok(FinanceOrderPreview {
        symbol,
        market,
        intent_direction,
        order_direction,
        approval_status,
        price: resolved_price,
        quantity,
        amount: resolved_price * quantity as f64,
        position_pct,
        order_type: "limit".into(),
        entry_rule: value_string(intent, "entry_rule"),
        thesis: value_string(intent, "thesis"),
        notes,
    })
}

#[tauri::command]
pub async fn finance_health(state: State<'_, AppState>) -> Result<FinanceHealthSnapshot, String> {
    let app_state = state.inner().clone();
    Ok(build_health_snapshot(&app_state).await)
}

#[tauri::command]
pub async fn finance_overview_snapshot(
    state: State<'_, AppState>,
) -> Result<FinanceOverviewSnapshot, String> {
    let app_state = state.inner().clone();
    let health = build_health_snapshot(&app_state).await;
    let market_status = finance_call(&app_state, "intel", "trading_sessions", json!({})).await?;
    let watchlist = finance_call(&app_state, "intel", "get_watchlist", json!({})).await?;
    let balance = finance_call(&app_state, "exec", "get_balance", json!({})).await?;
    let positions = finance_call(&app_state, "exec", "get_positions", json!({})).await?;
    let orders = finance_call(&app_state, "exec", "get_orders", json!({})).await.unwrap_or_else(|_| json!({ "orders": [] }));
    let trades = finance_call(&app_state, "exec", "get_today_trades", json!({})).await?;
    let pnl = finance_call(&app_state, "exec", "get_pnl", json!({ "period": "today" })).await?;

    Ok(FinanceOverviewSnapshot {
      health,
      market_status,
      balance,
      positions: value_array(&positions, "positions"),
      watchlist: value_array(&watchlist, "stocks"),
      pnl,
      active_order_count: value_array(&orders, "orders").len(),
      today_trade_count: value_array(&trades, "trades").len(),
    })
}

#[tauri::command]
pub async fn finance_orders_snapshot(
    state: State<'_, AppState>,
) -> Result<FinanceOrdersSnapshot, String> {
    let app_state = state.inner().clone();
    let balance = finance_call(&app_state, "exec", "get_balance", json!({})).await?;
    let positions = finance_call(&app_state, "exec", "get_positions", json!({})).await?;
    let orders = finance_call(&app_state, "exec", "get_orders", json!({})).await.unwrap_or_else(|_| json!({ "orders": [] }));
    let trades = finance_call(&app_state, "exec", "get_today_trades", json!({})).await?;
    let pnl = finance_call(&app_state, "exec", "get_pnl", json!({ "period": "today" })).await?;

    Ok(FinanceOrdersSnapshot {
        balance,
        positions: value_array(&positions, "positions"),
        orders: value_array(&orders, "orders"),
        trades: value_array(&trades, "trades"),
        pnl,
    })
}

#[tauri::command]
pub async fn finance_symbol_snapshot(
    state: State<'_, AppState>,
    symbol: String,
) -> Result<FinanceSymbolSnapshot, String> {
    let app_state = state.inner().clone();
    let symbol = symbol.trim().to_string();
    let news = finance_call(
        &app_state,
        "intel",
        "search_news",
        json!({
            "query": format!("{symbol} 最新公告 研报 新闻"),
            "symbol": symbol,
            "limit": 6
        }),
    )
    .await?;
    let tables = finance_call(
        &app_state,
        "intel",
        "query_data",
        json!({
            "query": format!("{symbol} 最新价 近5日收盘价 成交额 市盈率"),
            "symbol": symbol,
            "period": "5d",
            "metrics": ["close", "change_pct", "volume", "pe"]
        }),
    )
    .await?;

    Ok(FinanceSymbolSnapshot {
        symbol,
        news: value_array(&news, "items"),
        tables: value_array(&tables, "tables"),
    })
}

#[tauri::command]
pub async fn finance_get_watchlist(state: State<'_, AppState>) -> Result<Vec<Value>, String> {
    let app_state = state.inner().clone();
    let watchlist = finance_call(&app_state, "intel", "get_watchlist", json!({})).await?;
    Ok(value_array(&watchlist, "stocks"))
}

#[tauri::command]
pub async fn finance_update_watchlist(
    state: State<'_, AppState>,
    action: String,
    stock: String,
) -> Result<Value, String> {
    let app_state = state.inner().clone();
    finance_call(
        &app_state,
        "intel",
        "update_watchlist",
        json!({
            "action": action,
            "stock": stock,
        }),
    )
    .await
}

#[tauri::command]
pub async fn finance_search_news(
    state: State<'_, AppState>,
    query: String,
    symbol: Option<String>,
    limit: Option<u32>,
) -> Result<Value, String> {
    let app_state = state.inner().clone();
    finance_call(
        &app_state,
        "intel",
        "search_news",
        json!({
            "query": query,
            "symbol": symbol,
            "limit": limit.unwrap_or(20),
        }),
    )
    .await
}

#[tauri::command]
pub async fn finance_query_data(
    state: State<'_, AppState>,
    query: String,
    symbol: Option<String>,
    period: Option<String>,
    metrics: Option<Vec<String>>,
) -> Result<Value, String> {
    let app_state = state.inner().clone();
    finance_call(
        &app_state,
        "intel",
        "query_data",
        json!({
            "query": query,
            "symbol": symbol,
            "period": period,
            "metrics": metrics,
        }),
    )
    .await
}

#[tauri::command]
pub async fn finance_screen_stocks(
    state: State<'_, AppState>,
    query: String,
    limit: Option<u32>,
    sort_by: Option<String>,
    sort_desc: Option<bool>,
) -> Result<Value, String> {
    let app_state = state.inner().clone();
    finance_call(
        &app_state,
        "intel",
        "screen_stocks",
        json!({
            "query": query,
            "limit": limit.unwrap_or(50),
            "sort_by": sort_by,
            "sort_desc": sort_desc.unwrap_or(true),
        }),
    )
    .await
}

#[tauri::command]
pub async fn finance_batch_check(
    state: State<'_, AppState>,
    intents: Vec<Value>,
) -> Result<Value, String> {
    let app_state = state.inner().clone();
    finance_call(
        &app_state,
        "risk",
        "batch_check",
        json!({
            "intents": intents,
        }),
    )
    .await
}

#[tauri::command]
pub async fn finance_prepare_order(
    state: State<'_, AppState>,
    intent: Value,
) -> Result<FinanceOrderPreview, String> {
    let app_state = state.inner().clone();
    build_order_preview(&app_state, &intent).await
}

#[tauri::command]
pub async fn finance_place_order(
    state: State<'_, AppState>,
    intent: Value,
) -> Result<Value, String> {
    let app_state = state.inner().clone();
    let preview = build_order_preview(&app_state, &intent).await?;
    finance_call(
        &app_state,
        "exec",
        "place_order",
        json!({
            "symbol": preview.symbol,
            "direction": preview.order_direction,
            "quantity": preview.quantity,
            "price": preview.price,
            "order_type": preview.order_type,
        }),
    )
    .await
}
