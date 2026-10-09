use bunq_graph::{Bindings, connect, discover_accounts, execute, read_balances, settle};

mod graph;
mod reporting;
mod settings;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = settings::Settings::load()?;
    let graph = graph::Graph::load(settings.graph_config.clone())?;
    let bindings = Bindings::load(&settings.bindings_config, &graph.names())?;

    let connected = connect(
        &settings.installation_path,
        settings.api_key.as_deref(),
        &settings.base_url,
        &settings.user_agent,
        &settings.device_description,
    )
    .await?;

    let accounts =
        discover_accounts(&connected.client, connected.session.user_id(), &bindings).await?;
    let initial = read_balances(&connected.client, &accounts).await?;
    let target = graph.eval(&initial)?;
    let settlement = settle(&initial, &target, &accounts)?;

    if settlement.is_empty() {
        println!("No transfers needed; balances are already synchronized.");
        return Ok(());
    }

    let mut reporter = reporting::TransferReporter::new(&settlement);
    for transfer in &settlement {
        if settings.dry_run {
            reporter.report(transfer, &reporting::TransferStatus::Dry)?;
            continue;
        }

        reporter.report(transfer, &reporting::TransferStatus::Pending)?;
        match execute(&connected.client, transfer).await {
            Ok(()) => reporter.report(transfer, &reporting::TransferStatus::Sent)?,
            Err(error) => {
                reporter.report(transfer, &reporting::TransferStatus::Failed)?;
                return Err(error.into());
            }
        }
    }
    Ok(())
}
