use anyhow::Result;

pub async fn run() -> Result<()> {
    crate::tui::run().await
}
