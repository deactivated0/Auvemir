use auvemir_core::{
    model::{Media, MediaKind},
    queue::Queue,
};
fn item(name: &str) -> Media {
    Media::new(MediaKind::LocalMusic, "local", name, name)
}
#[test]
fn queue_moves_preserve_the_active_entry_in_every_direction() {
    for active in 0..5 {
        for from in 0..5 {
            for to in 0..5 {
                let mut q = Queue::default();
                q.replace((0..5).map(|i| item(&i.to_string())).collect(), active);
                let playing = q.current().unwrap().id.clone();
                q.move_item(from, to);
                assert_eq!(q.current().unwrap().id, playing);
                assert_eq!(q.items.len(), 5);
            }
        }
    }
}
#[test]
fn play_next_and_invalid_moves_keep_queue_consistent() {
    let mut q = Queue::default();
    q.insert_next(item("a"));
    q.insert_next(item("b"));
    assert_eq!(q.current().unwrap().title, "a");
    assert_eq!(q.items[1].title, "b");
    assert!(!q.move_item(20, 0));
    assert!(!q.move_item(0, 20));
    assert!(!q.move_item(0, 0));
    q.move_item(0, 1);
    assert_eq!(q.cursor, 1);
    assert_eq!(q.current().unwrap().title, "a");
    q.insert_next(item("c"));
    assert_eq!(q.items[2].title, "c");
}
