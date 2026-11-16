//! Steam wallet and purchase history (store.steampowered.com/account/history/): date, what was bought,
//! method, amount, wallet change. View only.

use aurelia::steam_client::SteamClient;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WalletRow {
    pub date: String,
    pub items: String,
    pub kind: String,
    pub total: String,
    pub change: String,
}

pub async fn load(client: &SteamClient) -> Result<Vec<WalletRow>, String> {
    let web = crate::webapi::web_session(client).await?;
    let html = web
        .get_text("https://store.steampowered.com/account/history/?l=russian")
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(parse(&html))
}

fn cell(row: &str, class: &str) -> String {
    let Some(start) = row.find(&format!("class=\"{class}")) else { return String::new() };
    let rest = &row[start..];
    let Some(gt) = rest.find('>') else { return String::new() };
    let body = &rest[gt + 1..];
    let end = body.find("</td>").unwrap_or(body.len());
    crate::profile_extra::strip_tags(&body[..end])
}

/// Rows of the wallet_history_table: wht_date, wht_items, wht_type, wht_total, wht_wallet_change.
pub fn parse(html: &str) -> Vec<WalletRow> {
    html.split("class=\"wallet_table_row")
        .skip(1)
        .map(|row| {
            let row = row.split("</tr>").next().unwrap_or(row);
            WalletRow {
                date: cell(row, "wht_date"),
                items: cell(row, "wht_items"),
                kind: cell(row, "wht_type"),
                total: cell(row, "wht_total"),
                change: cell(row, "wht_wallet_change"),
            }
        })
        .filter(|r| !r.date.is_empty() || !r.items.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_rows() {
        let html = r#"<table class="wallet_history_table"><tr class="wallet_table_row wallet_table_row_amt_change">
            <td class="wht_date">12 ноя. 2026</td><td class="wht_items"><div>Half-Life</div></td>
            <td class="wht_type"><div>Покупка</div><div class="wth_payment">Кошелек Steam</div></td>
            <td class="wht_total">10,00₴</td><td class="wht_wallet_change wallet_column">-10,00₴</td>
            <td class="wht_wallet_balance wallet_column">0,81₴</td></tr></table>"#;
        let r = super::parse(html);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].items, "Half-Life");
        assert_eq!(r[0].total, "10,00₴");
        assert_eq!(r[0].change, "-10,00₴");
        assert!(r[0].kind.starts_with("Покупка"));
    }

    /// Live parsing check (read only, own account): row count and the first fields.
    #[test]
    #[ignore]
    fn wallet_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let rows = super::load(&c).await.unwrap();
            println!("строк: {}", rows.len());
            if let Some(r) = rows.first() {
                println!("первая: дата '{}', тип '{}', есть сумма {}", r.date, r.kind, !r.total.is_empty());
            }
        });
    }
}
