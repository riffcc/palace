//! Test Zulip message management: slots, polls, and cleanup.
//!
//! Demonstrates:
//! - Status/Tasks slots (persistent, editable)
//! - Progress/Poll/Blocker slots (transient, cleaned up)
//! - Native polls for decisions (replaced, not spammed)
//!
//! Usage:
//!   cargo run -p director --example widgets

use director::{ZulipReporter, TodoTask};
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::from_path(std::path::Path::new(&std::env::var("HOME")?).join("ai/zulip/.env"));

    let mut reporter = ZulipReporter::from_env()?;
    let session_id = Uuid::new_v4();
    let session_name = "slot-test";

    println!("Testing Zulip message slot system...\n");

    // 1. Initialize session with status and tasks
    println!("1. Initializing session with status and task list...");
    reporter.init_session_with_tasks(
        session_id,
        session_name,
        "test:widgets",
        &["Analyze requirements", "Create implementation", "Write tests", "Update docs"],
    ).await?;
    println!("   ✓ Status message created");
    println!("   ✓ Task list created");

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // 2. Update progress (same message, edited in place)
    println!("\n2. Updating progress (edits existing message)...");
    reporter.session_progress(session_id, session_name, 1, 4, "Analyzing requirements").await?;
    println!("   ✓ Progress updated");

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // 3. Send a poll (replaces any previous poll)
    println!("\n3. Sending poll (replaces previous if exists)...");
    reporter.poll(
        session_id,
        session_name,
        "Which approach should we use?",
        &["Quick prototype", "Full implementation", "Research first"],
    ).await?;
    println!("   ✓ Poll sent");

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // 4. Send another poll (replaces the first one, doesn't spam)
    println!("\n4. Sending another poll (replaces previous)...");
    reporter.poll(
        session_id,
        session_name,
        "Ready to proceed with implementation?",
        &["Yes, go ahead", "Need more context", "Wait for review"],
    ).await?;
    println!("   ✓ New poll replaced old one");

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // 5. Update task list (marks first task done)
    println!("\n5. Updating task list (marking task complete)...");
    let mut tasks = vec![
        TodoTask::completed("Analyze requirements"),
        TodoTask::new("Create implementation"),
        TodoTask::new("Write tests"),
        TodoTask::new("Update docs"),
    ];
    reporter.update_tasks(session_id, session_name, "Tasks", &tasks).await?;
    println!("   ✓ Task list updated");

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // 6. Progress update (same slot, edited)
    println!("\n6. More progress (same message edited)...");
    reporter.session_progress(session_id, session_name, 2, 4, "Creating implementation").await?;
    println!("   ✓ Progress updated");

    // 7. Mark another task complete
    tasks[1].completed = true;
    reporter.update_tasks(session_id, session_name, "Tasks", &tasks).await?;

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // 8. Clear poll when answered
    println!("\n7. Clearing poll (simulating answer received)...");
    reporter.clear_poll(session_id).await?;
    println!("   ✓ Poll deleted");

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // 9. Session completed (cleans up transient messages)
    println!("\n8. Completing session (cleans up progress/blockers)...");
    tasks[2].completed = true;
    tasks[3].completed = true;
    reporter.update_tasks(session_id, session_name, "Tasks", &tasks).await?;
    reporter.session_completed(session_id, session_name, "All tasks finished successfully!").await?;
    println!("   ✓ Session completed, transient messages cleaned");

    println!("\n✅ Done! Check Zulip stream 'palace', topic 'session/{}...'", &session_id.to_string()[..8]);
    println!("\nMessage behavior:");
    println!("  - Status: Persists (updated to 'Completed')");
    println!("  - Tasks: Persists (all items checked)");
    println!("  - Progress: Deleted on completion");
    println!("  - Poll: Deleted when answered or on completion");

    Ok(())
}
