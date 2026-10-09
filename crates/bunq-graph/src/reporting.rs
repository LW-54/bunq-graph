use std::io::{self, IsTerminal, Write};

use bunq_graph::{Settlement, Transfer};

pub enum TransferStatus {
    Dry,
    Pending,
    Sent,
    Failed,
}

pub struct TransferReporter {
    widths: TransferWidths,
    terminal: bool,
    total: usize,
    ordinal: usize,
}

impl TransferReporter {
    #[must_use]
    pub fn new(settlement: &Settlement) -> Self {
        Self {
            widths: TransferWidths::from(settlement),
            terminal: io::stdout().is_terminal(),
            total: settlement.len(),
            ordinal: 0,
        }
    }

    pub fn report(&mut self, transfer: &Transfer, status: &TransferStatus) -> io::Result<()> {
        if matches!(status, TransferStatus::Pending) {
            if self.terminal {
                let ordinal = self.ordinal.saturating_add(1);
                let line = self.widths.render(ordinal, self.total, transfer, status);
                print!("\r{line}");
                io::stdout().flush()?;
            }
            return Ok(());
        }

        self.ordinal = self.ordinal.saturating_add(1);
        let line = self
            .widths
            .render(self.ordinal, self.total, transfer, status);
        if self.terminal {
            print!("\r{line}\x1b[K\n");
            io::stdout().flush()?;
        } else {
            println!("{line}");
        }
        Ok(())
    }
}

struct TransferWidths {
    source: usize,
    target: usize,
    amount: usize,
    counter: usize,
}

impl TransferWidths {
    fn from(settlement: &Settlement) -> Self {
        Self {
            source: settlement
                .iter()
                .map(|transfer| transfer.source.name.len())
                .max()
                .unwrap_or(0),
            target: settlement
                .iter()
                .map(|transfer| transfer.target.name.len())
                .max()
                .unwrap_or(0),
            amount: settlement
                .iter()
                .map(|transfer| transfer.amount_minor.to_string().len())
                .max()
                .unwrap_or(0),
            counter: settlement.len().to_string().len(),
        }
    }

    fn render(
        &self,
        ordinal: usize,
        total: usize,
        transfer: &Transfer,
        status: &TransferStatus,
    ) -> String {
        format!(
            "[{ordinal:>counter$}/{total:>counter$}] {source:<source_width$} -> {target:<target_width$} {amount:>amount_width$} {unit:<11} [{status:^7}]",
            ordinal = ordinal,
            total = total,
            counter = self.counter,
            source = transfer.source.name,
            source_width = self.source,
            target = transfer.target.name,
            target_width = self.target,
            amount = transfer.amount_minor,
            amount_width = self.amount,
            unit = "minor units",
            status = status,
        )
    }
}

impl std::fmt::Display for TransferStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Dry => "DRY",
            Self::Pending => "...",
            Self::Sent => "SENT",
            Self::Failed => "FAILED",
        };
        formatter.write_str(label)
    }
}
