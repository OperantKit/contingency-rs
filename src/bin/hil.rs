//! `contingency-hil` — Hardware-In-the-Loop bridge binary.
//!
//! Speaks the same JSONL TCP wire protocol consumed by the Python
//! `HilBridgeApparatus` on the other side. Concrete schedule-runner
//! wiring lands in Phase 6; this file currently holds the entry
//! point so the workspace builds cleanly during Phase 0.

fn main() {
    eprintln!("contingency-hil: Phase 6 implementation pending.");
    std::process::exit(2);
}
