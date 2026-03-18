use openpup_tauri::memory::system::MemorySystem;

#[tokio::test]
async fn add_and_list_long_term_memories_works() {
  let db_url = "sqlite::memory:";
  let system = MemorySystem::new(db_url).await.expect("init memory");

  system
    .add_long_term_memory("test fact", "fact", 0.9)
    .await
    .expect("insert");
  system
    .add_long_term_memory("another fact", "fact", 0.5)
    .await
    .expect("insert");

  let items = system
    .list_long_term_memories(0, 10, None)
    .await
    .expect("list");

  assert!(!items.is_empty());
  assert!(items.iter().any(|(_, content, _, _, _)| content == "test fact"));
}

