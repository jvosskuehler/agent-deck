//! Probelauf des Lagebilds gegen ein echtes Transcript: ruft wirklich `claude` (Sonnet),
//! schreibt aber nur in einen Wegwerf-Cache. Von Hand, um Prompt und Auszug zu beurteilen:
//!
//! ```powershell
//! $env:BRIEF_SID = "<session-id>"; $env:BRIEF_CWD = "<cwd der Session>"   # cwd darf fehlen
//! cargo test -p deck-services --test briefing_real -- --ignored --nocapture
//! ```

use deck_core::i18n::Lang;
use deck_services::summary::{SummaryConfig, SummaryService, ClaudeCli};
use std::sync::Arc;

#[test]
#[ignore = "ruft das echte claude auf; nur von Hand"]
fn a_real_transcript_gets_a_briefing() {
    let sid = std::env::var("BRIEF_SID").expect("BRIEF_SID setzen");
    let cwd = std::env::var("BRIEF_CWD").ok();
    let tmp = tempfile::tempdir().unwrap();
    let svc = SummaryService::new(SummaryConfig::standard(tmp.path()), Arc::new(ClaudeCli));
    let started = std::time::Instant::now();
    let short = svc.generate(&sid, cwd.as_deref(), Lang::German);
    println!("\n--- Kurzform ({:.1} s) ---\n{}", started.elapsed().as_secs_f64(), short.as_deref().unwrap_or("(keine)"));
    let (brief, _) = svc.cached_brief(&sid).expect("kein Lagebild: Transcript gefunden? Format getroffen?");
    println!("\n--- Lagebild ---\n{}\n", brief.lines("german"));
}
