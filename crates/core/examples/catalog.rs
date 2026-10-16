use anyhow::{Context, Result};
use auvemir_core::{model::MediaKind, providers::Providers};
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let p = Providers::new()?;
    match args.first().map(String::as_str) {
        Some("podcasts") => println!(
            "{}",
            serde_json::to_string_pretty(
                &p.podcasts(args.get(1).map(String::as_str).unwrap_or("Science"))
                    .await?
            )?
        ),
        Some("radio") => println!(
            "{}",
            serde_json::to_string_pretty(
                &p.radio(args.get(1).map(String::as_str).unwrap_or("BBC"))
                    .await?
            )?
        ),
        Some("books") => println!(
            "{}",
            serde_json::to_string_pretty(
                &p.books(args.get(1).map(String::as_str).unwrap_or("Alice"))
                    .await?
            )?
        ),
        Some("feed") => {
            let url = args.get(1).context("Enter an RSS URL")?;
            let (title, items) = p.feed(url, MediaKind::Podcast).await?;
            println!("{title}\n{}", serde_json::to_string_pretty(&items)?);
        }
        _ => anyhow::bail!(
            "Usage: cargo run -p auvemir-core --example catalog -- radio|books|feed QUERY_OR_URL"
        ),
    }
    Ok(())
}
