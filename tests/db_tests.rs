mod common;

use flo::db;
use flo::models::{CreateTask, CreateSample, UpdateTask};

// ═══════════════════════════════════════════════════════════════════
// Fork DB tests — verify isolation from production
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn fork_db_has_production_data() {
    let (pool, _tmp) = common::fork_db().await;
    let roots = db::get_children(&pool, None).await.unwrap();
    assert!(!roots.is_empty(), "forked db should have production data");
}

#[tokio::test]
async fn fork_db_mutations_dont_affect_production() {
    let (pool, _tmp) = common::fork_db().await;
    let before = db::get_children(&pool, None).await.unwrap();
    let count_before = before.len();

    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "TEST_TASK_SHOULD_NOT_EXIST_IN_PROD".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let after = db::get_children(&pool, None).await.unwrap();
    assert_eq!(after.len(), count_before + 1);
    db::delete_task(&pool, &task.id).await.unwrap();
}

#[tokio::test]
async fn empty_db_creates_fresh_schema() {
    let (pool, _tmp) = common::empty_db().await;
    let roots = db::get_children(&pool, None).await.unwrap();
    assert!(roots.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// Task CRUD — basics
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_and_get_task() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "test task".to_string(),
        notes: "some notes".to_string(),
    })
    .await
    .unwrap();

    assert_eq!(task.title, "test task");
    assert_eq!(task.notes, "some notes");
    assert!(!task.completed);
    assert!(task.parent_id.is_none());

    let fetched = db::get_task(&pool, &task.id).await.unwrap();
    assert_eq!(fetched.id, task.id);
    assert_eq!(fetched.title, "test task");
}

#[tokio::test]
async fn create_task_with_empty_title() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();
    assert_eq!(task.title, "");
}

#[tokio::test]
async fn create_task_with_unicode() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "买菜 🛒 café résumé".to_string(),
        notes: "日本語のノート\n\temoji: 🎉🔥".to_string(),
    })
    .await
    .unwrap();
    assert_eq!(task.title, "买菜 🛒 café résumé");
    assert!(task.notes.contains("🎉🔥"));
}

#[tokio::test]
async fn create_task_with_long_title() {
    let (pool, _tmp) = common::empty_db().await;
    let long_title = "a".repeat(10_000);
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: long_title.clone(),
        notes: String::new(),
    })
    .await
    .unwrap();
    assert_eq!(task.title.len(), 10_000);
}

#[tokio::test]
async fn create_task_with_special_chars() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: r#"title with "quotes" and 'apostrophes' and \backslash"#.to_string(),
        notes: "notes with\nnewlines\tand\ttabs".to_string(),
    })
    .await
    .unwrap();
    assert!(task.title.contains("\"quotes\""));
    assert!(task.notes.contains("\n"));
}

#[tokio::test]
async fn create_task_multiline_notes() {
    let (pool, _tmp) = common::empty_db().await;
    let notes = "# Heading\n\n- bullet 1\n- bullet 2\n\n```rust\nfn main() {}\n```";
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "has markdown".to_string(),
        notes: notes.to_string(),
    })
    .await
    .unwrap();
    assert_eq!(task.notes, notes);
}

#[tokio::test]
async fn get_nonexistent_task_fails() {
    let (pool, _tmp) = common::empty_db().await;
    let result = db::get_task(&pool, "nonexistent-id").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn task_has_ulid_format_id() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "check id".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();
    // ULIDs are 26 chars, uppercase alphanumeric
    assert_eq!(task.id.len(), 26);
    assert!(task.id.chars().all(|c| c.is_ascii_alphanumeric()));
}

#[tokio::test]
async fn task_has_timestamps() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "check timestamps".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();
    assert!(!task.created_at.is_empty());
    assert!(!task.updated_at.is_empty());
    assert_eq!(task.created_at, task.updated_at);
}

#[tokio::test]
async fn task_ids_are_unique() {
    let (pool, _tmp) = common::empty_db().await;
    let mut ids = std::collections::HashSet::new();
    for i in 0..50 {
        let task = db::create_task(&pool, &CreateTask {
            parent_id: None,
            title: format!("task {}", i),
            notes: String::new(),
        })
        .await
        .unwrap();
        assert!(ids.insert(task.id), "duplicate task ID generated");
    }
}

// ═══════════════════════════════════════════════════════════════════
// Parent-child relationships
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_child_task() {
    let (pool, _tmp) = common::empty_db().await;
    let parent = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "parent".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let child = db::create_task(&pool, &CreateTask {
        parent_id: Some(parent.id.clone()),
        title: "child".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    let children = db::get_children(&pool, Some(&parent.id)).await.unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, child.id);
}

#[tokio::test]
async fn deeply_nested_hierarchy() {
    let (pool, _tmp) = common::empty_db().await;
    let mut parent_id: Option<String> = None;
    let mut ids = Vec::new();

    for i in 0..10 {
        let task = db::create_task(&pool, &CreateTask {
            parent_id: parent_id.clone(),
            title: format!("level {}", i),
            notes: String::new(),
        })
        .await
        .unwrap();
        ids.push(task.id.clone());
        parent_id = Some(task.id);
    }

    // Verify the chain
    for (i, id) in ids.iter().enumerate() {
        let task = db::get_task(&pool, id).await.unwrap();
        if i == 0 {
            assert!(task.parent_id.is_none());
        } else {
            assert_eq!(task.parent_id.as_deref(), Some(ids[i - 1].as_str()));
        }
    }
}

#[tokio::test]
async fn get_children_empty() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "lonely".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let children = db::get_children(&pool, Some(&task.id)).await.unwrap();
    assert!(children.is_empty());
}

#[tokio::test]
async fn get_children_of_root() {
    let (pool, _tmp) = common::empty_db().await;
    for i in 0..5 {
        db::create_task(&pool, &CreateTask {
            parent_id: None,
            title: format!("root {}", i),
            notes: String::new(),
        })
        .await
        .unwrap();
    }
    let roots = db::get_children(&pool, None).await.unwrap();
    assert_eq!(roots.len(), 5);
}

#[tokio::test]
async fn multiple_children_same_parent() {
    let (pool, _tmp) = common::empty_db().await;
    let parent = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "parent".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    for i in 0..10 {
        db::create_task(&pool, &CreateTask {
            parent_id: Some(parent.id.clone()),
            title: format!("child {}", i),
            notes: String::new(),
        })
        .await
        .unwrap();
    }

    let children = db::get_children(&pool, Some(&parent.id)).await.unwrap();
    assert_eq!(children.len(), 10);
}

#[tokio::test]
async fn get_task_with_children() {
    let (pool, _tmp) = common::empty_db().await;
    let parent = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "parent".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(parent.id.clone()),
        title: "child A".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(parent.id.clone()),
        title: "child B".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let tw = db::get_task_with_children(&pool, &parent.id).await.unwrap();
    assert_eq!(tw.task.title, "parent");
    assert_eq!(tw.children.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════
// Update
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn update_task_title() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "original".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let updated = db::update_task(&pool, &task.id, &UpdateTask {
        title: Some("renamed".to_string()),
        ..Default::default()
    })
    .await
    .unwrap();

    assert_eq!(updated.title, "renamed");
    assert_eq!(updated.notes, ""); // unchanged
    assert!(!updated.completed);   // unchanged
}

#[tokio::test]
async fn update_task_notes() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "t".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let updated = db::update_task(&pool, &task.id, &UpdateTask {
        notes: Some("new notes".to_string()),
        ..Default::default()
    })
    .await
    .unwrap();

    assert_eq!(updated.notes, "new notes");
    assert_eq!(updated.title, "t"); // unchanged
}

#[tokio::test]
async fn update_task_completed() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "t".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();
    assert!(!task.completed);

    let updated = db::update_task(&pool, &task.id, &UpdateTask {
        completed: Some(true),
        ..Default::default()
    })
    .await
    .unwrap();
    assert!(updated.completed);

    // Toggle back
    let toggled = db::update_task(&pool, &task.id, &UpdateTask {
        completed: Some(false),
        ..Default::default()
    })
    .await
    .unwrap();
    assert!(!toggled.completed);
}

#[tokio::test]
async fn update_task_all_fields() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "original".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let updated = db::update_task(&pool, &task.id, &UpdateTask {
        title: Some("renamed".to_string()),
        notes: Some("new notes".to_string()),
        completed: Some(true),
        position: Some(99),
        ..Default::default()
    })
    .await
    .unwrap();

    assert_eq!(updated.title, "renamed");
    assert_eq!(updated.notes, "new notes");
    assert!(updated.completed);
    assert_eq!(updated.position, 99);
}

#[tokio::test]
async fn update_task_empty_update_preserves_fields() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "keep me".to_string(),
        notes: "keep these".to_string(),
    })
    .await
    .unwrap();

    let updated = db::update_task(&pool, &task.id, &UpdateTask::default()).await.unwrap();
    assert_eq!(updated.title, "keep me");
    assert_eq!(updated.notes, "keep these");
}

#[tokio::test]
async fn update_task_updates_timestamp() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "t".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let original_updated_at = task.updated_at.clone();

    // Small delay to ensure different timestamp
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let updated = db::update_task(&pool, &task.id, &UpdateTask {
        title: Some("changed".to_string()),
        ..Default::default()
    })
    .await
    .unwrap();

    assert_ne!(updated.updated_at, original_updated_at);
    assert_eq!(updated.created_at, task.created_at); // created_at unchanged
}

#[tokio::test]
async fn update_nonexistent_task_fails() {
    let (pool, _tmp) = common::empty_db().await;
    let result = db::update_task(&pool, "nonexistent", &UpdateTask {
        title: Some("nope".to_string()),
        ..Default::default()
    })
    .await;
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════
// Reparenting
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn reparent_task() {
    let (pool, _tmp) = common::empty_db().await;
    let a = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "A".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let b = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "B".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    db::update_task(&pool, &b.id, &UpdateTask {
        parent_id: Some(a.id.clone()),
        ..Default::default()
    })
    .await
    .unwrap();

    let roots = db::get_children(&pool, None).await.unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].title, "A");

    let children = db::get_children(&pool, Some(&a.id)).await.unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].title, "B");
}

#[tokio::test]
async fn reparent_to_root_with_empty_string() {
    let (pool, _tmp) = common::empty_db().await;
    let parent = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "parent".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let child = db::create_task(&pool, &CreateTask {
        parent_id: Some(parent.id.clone()),
        title: "child".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    // parent_id = Some("") means set to root (NULL)
    db::update_task(&pool, &child.id, &UpdateTask {
        parent_id: Some("".to_string()),
        ..Default::default()
    })
    .await
    .unwrap();

    let roots = db::get_children(&pool, None).await.unwrap();
    assert_eq!(roots.len(), 2);
}

#[tokio::test]
async fn reparent_preserves_other_fields() {
    let (pool, _tmp) = common::empty_db().await;
    let a = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "A".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let b = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "B".to_string(),
        notes: "important notes".to_string(),
    })
    .await
    .unwrap();

    let updated = db::update_task(&pool, &b.id, &UpdateTask {
        parent_id: Some(a.id.clone()),
        ..Default::default()
    })
    .await
    .unwrap();

    assert_eq!(updated.title, "B");
    assert_eq!(updated.notes, "important notes");
}

// ═══════════════════════════════════════════════════════════════════
// Delete
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn delete_leaf_task() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "delete me".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    db::delete_task(&pool, &task.id).await.unwrap();
    let result = db::get_task(&pool, &task.id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn delete_task_cascades() {
    let (pool, _tmp) = common::empty_db().await;
    let parent = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "parent".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let child = db::create_task(&pool, &CreateTask {
        parent_id: Some(parent.id.clone()),
        title: "child".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let grandchild = db::create_task(&pool, &CreateTask {
        parent_id: Some(child.id.clone()),
        title: "grandchild".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    db::delete_task(&pool, &parent.id).await.unwrap();

    assert!(db::get_task(&pool, &parent.id).await.is_err());
    assert!(db::get_task(&pool, &child.id).await.is_err());
    assert!(db::get_task(&pool, &grandchild.id).await.is_err());
}

#[tokio::test]
async fn delete_middle_node_cascades_subtree() {
    let (pool, _tmp) = common::empty_db().await;
    let root = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "root".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let mid = db::create_task(&pool, &CreateTask {
        parent_id: Some(root.id.clone()),
        title: "mid".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let leaf = db::create_task(&pool, &CreateTask {
        parent_id: Some(mid.id.clone()),
        title: "leaf".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    let sibling = db::create_task(&pool, &CreateTask {
        parent_id: Some(root.id.clone()),
        title: "sibling".to_string(),
        notes: String::new(),
    })
    .await
    .unwrap();

    // Delete mid — should cascade to leaf, but sibling survives
    db::delete_task(&pool, &mid.id).await.unwrap();

    assert!(db::get_task(&pool, &mid.id).await.is_err());
    assert!(db::get_task(&pool, &leaf.id).await.is_err());
    assert!(db::get_task(&pool, &root.id).await.is_ok());
    assert!(db::get_task(&pool, &sibling.id).await.is_ok());
}

#[tokio::test]
async fn delete_nonexistent_task_succeeds() {
    let (pool, _tmp) = common::empty_db().await;
    // SQLite DELETE with no matching rows is not an error
    let result = db::delete_task(&pool, "nonexistent").await;
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// Position / ordering
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn position_auto_increments() {
    let (pool, _tmp) = common::empty_db().await;
    let t1 = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "first".to_string(),
        notes: String::new(),
    }).await.unwrap();

    let t2 = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "second".to_string(),
        notes: String::new(),
    }).await.unwrap();

    let t3 = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "third".to_string(),
        notes: String::new(),
    }).await.unwrap();

    assert!(t1.position < t2.position);
    assert!(t2.position < t3.position);
}

#[tokio::test]
async fn children_ordered_by_position() {
    let (pool, _tmp) = common::empty_db().await;
    let parent = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "parent".to_string(),
        notes: String::new(),
    }).await.unwrap();

    for i in 0..5 {
        db::create_task(&pool, &CreateTask {
            parent_id: Some(parent.id.clone()),
            title: format!("child {}", i),
            notes: String::new(),
        }).await.unwrap();
    }

    let children = db::get_children(&pool, Some(&parent.id)).await.unwrap();
    for i in 0..4 {
        assert!(children[i].position < children[i + 1].position);
    }
}

#[tokio::test]
async fn position_swap_reorders() {
    let (pool, _tmp) = common::empty_db().await;
    let t1 = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "first".to_string(),
        notes: String::new(),
    }).await.unwrap();

    let t2 = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "second".to_string(),
        notes: String::new(),
    }).await.unwrap();

    // Swap positions
    let pos1 = t1.position;
    let pos2 = t2.position;

    db::update_task(&pool, &t1.id, &UpdateTask {
        position: Some(pos2),
        ..Default::default()
    }).await.unwrap();

    db::update_task(&pool, &t2.id, &UpdateTask {
        position: Some(pos1),
        ..Default::default()
    }).await.unwrap();

    let tasks = db::get_children(&pool, None).await.unwrap();
    assert_eq!(tasks[0].title, "second");
    assert_eq!(tasks[1].title, "first");
}

#[tokio::test]
async fn sibling_positions_independent_across_parents() {
    let (pool, _tmp) = common::empty_db().await;
    let p1 = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "parent 1".to_string(),
        notes: String::new(),
    }).await.unwrap();

    let p2 = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "parent 2".to_string(),
        notes: String::new(),
    }).await.unwrap();

    let c1 = db::create_task(&pool, &CreateTask {
        parent_id: Some(p1.id.clone()),
        title: "child of p1".to_string(),
        notes: String::new(),
    }).await.unwrap();

    let c2 = db::create_task(&pool, &CreateTask {
        parent_id: Some(p2.id.clone()),
        title: "child of p2".to_string(),
        notes: String::new(),
    }).await.unwrap();

    // Both should start at position 0
    assert_eq!(c1.position, 0);
    assert_eq!(c2.position, 0);
}

// ═══════════════════════════════════════════════════════════════════
// Ancestors
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn get_ancestors_single_root() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "root".to_string(),
        notes: String::new(),
    }).await.unwrap();

    let ancestors = db::get_ancestors(&pool, &task.id).await.unwrap();
    assert_eq!(ancestors.len(), 1);
    assert_eq!(ancestors[0].id, task.id);
}

#[tokio::test]
async fn get_ancestors_deep_chain() {
    let (pool, _tmp) = common::empty_db().await;

    let root = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "root".into(), notes: String::new(),
    }).await.unwrap();

    let mid = db::create_task(&pool, &CreateTask {
        parent_id: Some(root.id.clone()), title: "mid".into(), notes: String::new(),
    }).await.unwrap();

    let leaf = db::create_task(&pool, &CreateTask {
        parent_id: Some(mid.id.clone()), title: "leaf".into(), notes: String::new(),
    }).await.unwrap();

    let ancestors = db::get_ancestors(&pool, &leaf.id).await.unwrap();
    assert_eq!(ancestors.len(), 3);
    assert_eq!(ancestors[0].title, "root");
    assert_eq!(ancestors[1].title, "mid");
    assert_eq!(ancestors[2].title, "leaf");
}

// ═══════════════════════════════════════════════════════════════════
// Subtree
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn get_subtree_single_node() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "alone".into(), notes: String::new(),
    }).await.unwrap();

    let subtree = db::get_subtree(&pool, &task.id).await.unwrap();
    assert_eq!(subtree.len(), 1);
    assert_eq!(subtree[0].title, "alone");
}

#[tokio::test]
async fn get_subtree_full_tree() {
    let (pool, _tmp) = common::empty_db().await;

    let root = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "root".into(), notes: String::new(),
    }).await.unwrap();

    let a = db::create_task(&pool, &CreateTask {
        parent_id: Some(root.id.clone()), title: "A".into(), notes: String::new(),
    }).await.unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(root.id.clone()), title: "B".into(), notes: String::new(),
    }).await.unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(a.id.clone()), title: "A1".into(), notes: String::new(),
    }).await.unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(a.id.clone()), title: "A2".into(), notes: String::new(),
    }).await.unwrap();

    let subtree = db::get_subtree(&pool, &root.id).await.unwrap();
    assert_eq!(subtree.len(), 5);
    // DFS order: root, A, B (depth 1), A1, A2 (depth 2) — ordered by depth then position
    assert_eq!(subtree[0].title, "root");
}

#[tokio::test]
async fn get_subtree_from_middle() {
    let (pool, _tmp) = common::empty_db().await;

    let root = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "root".into(), notes: String::new(),
    }).await.unwrap();

    let mid = db::create_task(&pool, &CreateTask {
        parent_id: Some(root.id.clone()), title: "mid".into(), notes: String::new(),
    }).await.unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(mid.id.clone()), title: "leaf".into(), notes: String::new(),
    }).await.unwrap();

    // Subtree from mid should NOT include root
    let subtree = db::get_subtree(&pool, &mid.id).await.unwrap();
    assert_eq!(subtree.len(), 2);
    assert_eq!(subtree[0].title, "mid");
    assert_eq!(subtree[1].title, "leaf");
}

// ═══════════════════════════════════════════════════════════════════
// Pending children
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn get_pending_children_filters_completed() {
    let (pool, _tmp) = common::empty_db().await;
    let parent = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "parent".into(), notes: String::new(),
    }).await.unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(parent.id.clone()), title: "pending".into(), notes: String::new(),
    }).await.unwrap();

    let done = db::create_task(&pool, &CreateTask {
        parent_id: Some(parent.id.clone()), title: "done".into(), notes: String::new(),
    }).await.unwrap();
    db::update_task(&pool, &done.id, &UpdateTask {
        completed: Some(true), ..Default::default()
    }).await.unwrap();

    let pending = db::get_pending_children(&pool, Some(&parent.id)).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].title, "pending");

    let all = db::get_children(&pool, Some(&parent.id)).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn get_pending_children_at_root() {
    let (pool, _tmp) = common::empty_db().await;

    db::create_task(&pool, &CreateTask {
        parent_id: None, title: "active".into(), notes: String::new(),
    }).await.unwrap();

    let done = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "done".into(), notes: String::new(),
    }).await.unwrap();
    db::update_task(&pool, &done.id, &UpdateTask {
        completed: Some(true), ..Default::default()
    }).await.unwrap();

    let pending = db::get_pending_children(&pool, None).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].title, "active");
}

// ═══════════════════════════════════════════════════════════════════
// Home / ProjectPreview
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn home_empty_db() {
    let (pool, _tmp) = common::empty_db().await;
    let home = db::get_home(&pool).await.unwrap();
    assert!(home.is_empty());
}

#[tokio::test]
async fn home_shows_pending_counts() {
    let (pool, _tmp) = common::empty_db().await;
    let project = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "My Project".into(), notes: String::new(),
    }).await.unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(project.id.clone()), title: "pending 1".into(), notes: String::new(),
    }).await.unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(project.id.clone()), title: "pending 2".into(), notes: String::new(),
    }).await.unwrap();

    let done = db::create_task(&pool, &CreateTask {
        parent_id: Some(project.id.clone()), title: "done".into(), notes: String::new(),
    }).await.unwrap();
    db::update_task(&pool, &done.id, &UpdateTask {
        completed: Some(true), ..Default::default()
    }).await.unwrap();

    let home = db::get_home(&pool).await.unwrap();
    assert_eq!(home.len(), 1);
    assert_eq!(home[0].pending_count, 2);
}

#[tokio::test]
async fn home_counts_nested_pending() {
    let (pool, _tmp) = common::empty_db().await;
    let project = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "proj".into(), notes: String::new(),
    }).await.unwrap();

    let sub = db::create_task(&pool, &CreateTask {
        parent_id: Some(project.id.clone()), title: "sub".into(), notes: String::new(),
    }).await.unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(sub.id.clone()), title: "deep task".into(), notes: String::new(),
    }).await.unwrap();

    let home = db::get_home(&pool).await.unwrap();
    assert_eq!(home[0].pending_count, 2); // sub + deep task
}

#[tokio::test]
async fn home_next_actions_limited_to_two() {
    let (pool, _tmp) = common::empty_db().await;
    let project = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "proj".into(), notes: String::new(),
    }).await.unwrap();

    for i in 0..5 {
        db::create_task(&pool, &CreateTask {
            parent_id: Some(project.id.clone()),
            title: format!("task {}", i),
            notes: String::new(),
        }).await.unwrap();
    }

    let home = db::get_home(&pool).await.unwrap();
    assert!(home[0].next_actions.len() <= 2);
}

#[tokio::test]
async fn home_next_actions_skips_completed() {
    let (pool, _tmp) = common::empty_db().await;
    let project = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "proj".into(), notes: String::new(),
    }).await.unwrap();

    let first = db::create_task(&pool, &CreateTask {
        parent_id: Some(project.id.clone()), title: "done first".into(), notes: String::new(),
    }).await.unwrap();
    db::update_task(&pool, &first.id, &UpdateTask {
        completed: Some(true), ..Default::default()
    }).await.unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(project.id.clone()), title: "pending".into(), notes: String::new(),
    }).await.unwrap();

    let home = db::get_home(&pool).await.unwrap();
    assert_eq!(home[0].next_actions.len(), 1);
    assert_eq!(home[0].next_actions[0].title, "pending");
}

// ═══════════════════════════════════════════════════════════════════
// Resolve ID prefix
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn resolve_id_full_match() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "t".into(), notes: String::new(),
    }).await.unwrap();

    let resolved = db::resolve_id(&pool, &task.id).await.unwrap();
    assert_eq!(resolved, task.id);
}

#[tokio::test]
async fn resolve_id_prefix_match() {
    let (pool, _tmp) = common::empty_db().await;
    let task = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "t".into(), notes: String::new(),
    }).await.unwrap();

    // First 8 chars should be unique enough in a single-task db
    let prefix = &task.id[..8];
    let resolved = db::resolve_id(&pool, prefix).await.unwrap();
    assert_eq!(resolved, task.id);
}

#[tokio::test]
async fn resolve_id_no_match() {
    let (pool, _tmp) = common::empty_db().await;
    let result = db::resolve_id(&pool, "ZZZZZZZZ").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no task found"));
}

// ═══════════════════════════════════════════════════════════════════
// Search
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn search_by_title() {
    let (pool, _tmp) = common::empty_db().await;
    db::create_task(&pool, &CreateTask {
        parent_id: None, title: "buy groceries".into(), notes: String::new(),
    }).await.unwrap();
    db::create_task(&pool, &CreateTask {
        parent_id: None, title: "unrelated".into(), notes: String::new(),
    }).await.unwrap();

    let results = db::search_tasks(&pool, "grocer").await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].task.title, "buy groceries");
}

#[tokio::test]
async fn search_by_notes() {
    let (pool, _tmp) = common::empty_db().await;
    db::create_task(&pool, &CreateTask {
        parent_id: None,
        title: "task".into(),
        notes: "mentions groceries here".into(),
    }).await.unwrap();

    let results = db::search_tasks(&pool, "grocer").await.unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn search_case_insensitive() {
    let (pool, _tmp) = common::empty_db().await;
    db::create_task(&pool, &CreateTask {
        parent_id: None, title: "Buy Groceries".into(), notes: String::new(),
    }).await.unwrap();

    let results = db::search_tasks(&pool, "buy").await.unwrap();
    assert_eq!(results.len(), 1);

    let results2 = db::search_tasks(&pool, "BUY").await.unwrap();
    assert_eq!(results2.len(), 1);
}

#[tokio::test]
async fn search_no_results() {
    let (pool, _tmp) = common::empty_db().await;
    db::create_task(&pool, &CreateTask {
        parent_id: None, title: "something".into(), notes: String::new(),
    }).await.unwrap();

    let results = db::search_tasks(&pool, "zzzznotfound").await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn search_includes_path() {
    let (pool, _tmp) = common::empty_db().await;
    let root = db::create_task(&pool, &CreateTask {
        parent_id: None, title: "Work".into(), notes: String::new(),
    }).await.unwrap();

    let mid = db::create_task(&pool, &CreateTask {
        parent_id: Some(root.id.clone()), title: "Project Alpha".into(), notes: String::new(),
    }).await.unwrap();

    db::create_task(&pool, &CreateTask {
        parent_id: Some(mid.id.clone()), title: "fix the bug".into(), notes: String::new(),
    }).await.unwrap();

    let results = db::search_tasks(&pool, "fix the bug").await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, vec!["Work", "Project Alpha"]);
}

#[tokio::test]
async fn search_root_task_has_empty_path() {
    let (pool, _tmp) = common::empty_db().await;
    db::create_task(&pool, &CreateTask {
        parent_id: None, title: "root task".into(), notes: String::new(),
    }).await.unwrap();

    let results = db::search_tasks(&pool, "root task").await.unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].path.is_empty());
}

#[tokio::test]
async fn search_limited_to_30() {
    let (pool, _tmp) = common::empty_db().await;
    for i in 0..50 {
        db::create_task(&pool, &CreateTask {
            parent_id: None, title: format!("findme {}", i), notes: String::new(),
        }).await.unwrap();
    }

    let results = db::search_tasks(&pool, "findme").await.unwrap();
    assert_eq!(results.len(), 30);
}

// ═══════════════════════════════════════════════════════════════════
// Samples (Mirror)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_sample() {
    let (pool, _tmp) = common::empty_db().await;
    let sample = db::create_sample(&pool, &CreateSample {
        response: "working on flo".into(),
        prompt_type: "activity".into(),
    }).await.unwrap();

    assert_eq!(sample.response, "working on flo");
    assert_eq!(sample.prompt_type, "activity");
    assert!(!sample.id.is_empty());
    assert!(!sample.created_at.is_empty());
}

#[tokio::test]
async fn create_sample_ping_type() {
    let (pool, _tmp) = common::empty_db().await;
    let sample = db::create_sample(&pool, &CreateSample {
        response: "reading a book".into(),
        prompt_type: "ping".into(),
    }).await.unwrap();

    assert_eq!(sample.prompt_type, "ping");
}

#[tokio::test]
async fn get_samples_today() {
    let (pool, _tmp) = common::empty_db().await;

    db::create_sample(&pool, &CreateSample {
        response: "sample 1".into(),
        prompt_type: "activity".into(),
    }).await.unwrap();

    db::create_sample(&pool, &CreateSample {
        response: "sample 2".into(),
        prompt_type: "ping".into(),
    }).await.unwrap();

    let samples = db::get_samples_today(&pool).await.unwrap();
    assert_eq!(samples.len(), 2);
}

#[tokio::test]
async fn samples_ordered_by_created_at() {
    let (pool, _tmp) = common::empty_db().await;

    let s1 = db::create_sample(&pool, &CreateSample {
        response: "first".into(),
        prompt_type: "activity".into(),
    }).await.unwrap();

    let s2 = db::create_sample(&pool, &CreateSample {
        response: "second".into(),
        prompt_type: "activity".into(),
    }).await.unwrap();

    let samples = db::get_samples_today(&pool).await.unwrap();
    assert!(samples[0].created_at <= samples[1].created_at);
}

#[tokio::test]
async fn get_samples_range() {
    let (pool, _tmp) = common::empty_db().await;

    db::create_sample(&pool, &CreateSample {
        response: "test".into(),
        prompt_type: "activity".into(),
    }).await.unwrap();

    // Range that includes today
    let from = "2020-01-01T00:00:00Z";
    let to = "2030-01-01T00:00:00Z";
    let samples = db::get_samples_range(&pool, from, to).await.unwrap();
    assert!(!samples.is_empty());

    // Range that excludes everything
    let old_from = "2020-01-01T00:00:00Z";
    let old_to = "2020-01-02T00:00:00Z";
    let none = db::get_samples_range(&pool, old_from, old_to).await.unwrap();
    assert!(none.is_empty());
}
