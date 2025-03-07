use crate::config::{Config, RssList};
use crate::notification::notify_all;
use rss::{Channel, Item};
use std::error::Error;
use transmission_rpc::types::{BasicAuth, RpcResponse, TorrentAddArgs, TorrentAdded};
use transmission_rpc::TransClient;

pub async fn process_feed(item: RssList, cfg: Config) -> Result<i32, Box<dyn Error + Send + Sync>> {
    println!("----------------------------");
    println!("==> Processing [{}]", item.title);

    // Open the database
    let db = sled::open(&cfg.persistence.path)?;

    // Fetch the url
    let content = reqwest::get(item.url).await?.bytes().await?;
    let channel = Channel::read_from(&content[..])?;

    // Creates a new connection
    let basic_auth = BasicAuth {
        user: cfg.transmission.username.clone(),
        password: cfg.transmission.password.clone(),
    };
    let client = TransClient::with_auth(&cfg.transmission.url, basic_auth);

    // Filters the results
    let results: Vec<&Item> = channel
        .items()
        .into_iter()
        .filter(|it| {
            // Check if item is already on db
            let db_found = match db.get(it.link().unwrap()) {
                Ok(val) => val,
                Err(_) => None,
            };

            if db_found.is_none() {
                let mut found = false;

                // If no filter is set just accept the item
                if item.filters.len() == 0 {
                    return true;
                }

                for filter in item.filters.clone() {
                    if it.title().unwrap_or_default().contains(&filter) {
                        found = true;
                    }
                }

                return found;
            }

            false
        })
        .collect();

    let mut count = 0;
    for result in results {
        let title = result.title().unwrap_or_default();
        let link = result.link().unwrap_or_default().to_string();
        // Add the torrent into transmission
        let add: TorrentAddArgs = TorrentAddArgs {
            filename: Some(link.clone()),
            download_dir: Some(item.download_dir.clone()),
            ..TorrentAddArgs::default()
        };
        let res: RpcResponse<TorrentAdded> = client.torrent_add(add).await?;
        if res.is_ok() {
            // Update counter
            count += 1;

            // Send notification
            notify_all(cfg.clone(), format!("Downloading: {}", title)).await;

            // Persist the item into the database
            match db.insert(&link, b"") {
                Ok(_) => println!("{:?} saved into db!", &link),
                Err(err) => println!("Failed to save {:?} into db: {:?}", link, err),
            }
        }
    }

    // Persist changes on disk
    db.flush()?;

    Ok(count)
}
