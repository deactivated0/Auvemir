use auvemir_core::{
    database::Store,
    model::{Media, MediaKind},
};
use std::collections::HashSet;

#[test]
fn batched_download_flags_match_individual_queries() {
    let mut db = Store::memory().unwrap();
    let tracks = (0..200)
        .map(|i| {
            Media::new(
                MediaKind::LocalMusic,
                "local",
                &format!("/music/{i} 'ع'.wav"),
                &format!("Track {i}"),
            )
        })
        .collect::<Vec<_>>();
    db.upsert(&tracks).unwrap();
    for m in tracks.iter().step_by(2) {
        db.attach_download(&m.id, &m.location).unwrap();
    }
    let mut ids = tracks.iter().map(|m| m.id.as_str()).collect::<Vec<_>>();
    ids.extend(["unknown", tracks[0].id.as_str()]);
    let expected = ids
        .iter()
        .filter(|id| db.download_path(id).unwrap().is_some())
        .map(|id| (*id).to_owned())
        .collect::<HashSet<_>>();
    assert_eq!(db.download_flags(&ids).unwrap(), expected);
    assert!(db.download_flags(&[]).unwrap().is_empty());
}
