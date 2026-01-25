use director::ZulipTool;
use std::sync::{Arc, Mutex};

/// Test the EXACT flow that pal next uses
/// Requires PALACE_ZULIP_* env vars to be set
#[tokio::test]
#[ignore = "requires Zulip credentials"]
async fn test_pal_next_zulip_flow() {
    // This is what pal next does
    let tool = ZulipTool::from_env_palace().expect("from_env_palace failed");

    // ensure_stream like pal next does
    let stream = "palace";
    tool.ensure_stream(stream).await.expect("ensure_stream failed");

    // send initial message like pal next does
    let initial = "🔍 **Exploring codebase...**\n\n";
    let msg_id = tool.send(stream, "suggestions", initial).await
        .expect("send failed");
    println!("Initial message posted: id={}", msg_id);

    // update like the callback does
    let updated = "🔍 **Exploring codebase...**\n\n📂 .\n📖 README.md\n";
    tool.update_message(msg_id, updated).await
        .expect("update_message failed");
    println!("Message updated");

    // verify
    let messages = tool.get_messages(stream, Some("suggestions"), 5).await
        .expect("get_messages failed");

    let our_msg = messages.iter().find(|m| m.id == msg_id);
    assert!(our_msg.is_some(), "Message {} not found", msg_id);
    println!("SUCCESS: Message found with content: {}", our_msg.unwrap().content);
}

#[tokio::test]
#[ignore = "requires Zulip credentials"]
async fn test_zulip_from_env_palace() {
    let result = ZulipTool::from_env_palace();
    match &result {
        Ok(_) => println!("SUCCESS: ZulipTool created"),
        Err(e) => println!("FAILED to create ZulipTool: {}", e),
    }
    assert!(result.is_ok(), "ZulipTool::from_env_palace() failed: {:?}", result.err());
}

#[tokio::test]
#[ignore = "requires Zulip credentials"]
async fn test_zulip_send() {
    let tool = ZulipTool::from_env_palace().expect("Failed to create ZulipTool");
    let result = tool.send("palace", "test", "Test message from cargo test").await;
    match &result {
        Ok(msg_id) => println!("SUCCESS: Sent message id={}", msg_id),
        Err(e) => println!("FAILED to send: {}", e),
    }
    assert!(result.is_ok(), "send() failed: {:?}", result.err());
}

#[tokio::test]
#[ignore = "requires Zulip credentials"]
async fn test_zulip_send_and_update() {
    let tool = ZulipTool::from_env_palace().expect("Failed to create ZulipTool");

    // Use unique content so we can verify
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let initial = format!("INITIAL_{}", timestamp);
    let updated = format!("UPDATED_{}", timestamp);

    let msg_id = tool.send("palace", "test", &initial).await
        .expect("Failed to send initial message");
    println!("Sent initial message: id={}, content={}", msg_id, initial);

    // Small delay to ensure message is posted
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let update_result = tool.update_message(msg_id, &updated).await;
    match &update_result {
        Ok(_) => println!("update_message returned OK"),
        Err(e) => println!("update_message FAILED: {}", e),
    }
    assert!(update_result.is_ok(), "update_message() failed: {:?}", update_result.err());

    // Small delay then fetch to verify
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let messages = tool.get_messages("palace", Some("test"), 5).await
        .expect("Failed to fetch messages");

    println!("\nRecent messages in palace/test:");
    for msg in &messages {
        println!("  id={}: {}", msg.id, msg.content.chars().take(60).collect::<String>());
    }

    // Find our message
    let our_msg = messages.iter().find(|m| m.id == msg_id);
    match our_msg {
        Some(m) => {
            println!("\nMessage {} content: {}", msg_id, m.content);
            assert!(m.content.contains(&updated),
                "Message should contain '{}' but has '{}'", updated, m.content);
            assert!(!m.content.contains(&initial),
                "Message should NOT contain initial content '{}' but has '{}'", initial, m.content);
            println!("SUCCESS: Message was properly edited!");
        }
        None => {
            panic!("Could not find message {} in recent messages!", msg_id);
        }
    }
}

/// Test the streaming pattern used by `pal next` - rapid fire updates from callback
/// Requires PALACE_ZULIP_* env vars to be set
#[tokio::test]
#[ignore = "requires Zulip credentials"]
async fn test_zulip_streaming_pattern() {
    let tool = ZulipTool::from_env_palace().expect("Failed to create ZulipTool");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Initial message like pal next does
    let initial = format!("🔍 **Exploring codebase... ({})**\n\n", timestamp);
    let msg_id = tool.send("palace", "test-streaming", &initial).await
        .expect("Failed to send initial message");
    println!("Initial message: id={}", msg_id);

    // Simulate callback updates (like pal next does with Arc<Mutex<>>)
    let state: Arc<Mutex<(ZulipTool, u64, String)>> = Arc::new(Mutex::new((tool.clone(), msg_id, initial)));

    // Simulate 5 rapid tool call events
    let events = vec![
        "📖 src/main.rs",
        "📂 src/",
        "🔍 searching for TODO",
        "📖 src/lib.rs",
        "💡 Generating suggestions...",
    ];

    for event in &events {
        // This mimics what the callback in main.rs does
        let state_clone = state.clone();
        let event = event.to_string();

        // Spawn thread like main.rs does
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            if let Ok(mut guard) = state_clone.lock() {
                let (ref tool, msg_id, ref mut content) = *guard;
                content.push_str(&format!("{}\n", event));
                let tool = tool.clone();
                let content = content.clone();
                let _ = rt.block_on(tool.update_message(msg_id, &content));
            }
        }).join().unwrap(); // Wait for each to complete for determinism

        // Small delay between updates
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Final delay then check
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let messages = tool.get_messages("palace", Some("test-streaming"), 3).await
        .expect("Failed to fetch messages");

    println!("\nMessages in test-streaming:");
    for msg in &messages {
        println!("  id={}: {}", msg.id, &msg.content[..msg.content.len().min(100)]);
    }

    let our_msg = messages.iter().find(|m| m.id == msg_id);
    match our_msg {
        Some(m) => {
            println!("\nFinal message content:\n{}", m.content);
            // Verify all events made it
            for event in &events {
                assert!(m.content.contains(event),
                    "Message should contain '{}' but doesn't", event);
            }
            println!("\nSUCCESS: All {} events streamed correctly!", events.len());
        }
        None => {
            panic!("Could not find message {} in recent messages!", msg_id);
        }
    }
}
