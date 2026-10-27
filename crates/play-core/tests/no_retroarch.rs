use play_core::Play;
use std::fs;

#[test]
fn a_launch_with_no_emulator_says_where_it_looked_and_records_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("no-retroarch-here");
    std::env::set_var("RETROARCH", &missing);

    let drop = tmp.path().join("downloads");
    fs::create_dir_all(&drop).unwrap();
    fs::write(drop.join("Mario Kart 64 (USA).z64"), {
        let mut rom = vec![0u8; 4096];
        rom[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        rom
    })
    .unwrap();

    let play = Play::open(&tmp.path().join("play")).unwrap();
    play.intake(&drop, None).unwrap();
    let game = play.db().list_games("", None).unwrap().remove(0);
    assert_eq!(game.system, "N64");

    let status = play.retroarch_status();
    assert!(!status.available);
    assert!(status.path.is_none());
    let problem = status.problem.clone().expect("a reason to show");
    assert!(
        problem.contains("no-retroarch-here"),
        "the reason should name the setting that is wrong: {problem}"
    );
    assert!(
        !status.searched.is_empty(),
        "the interface has nowhere to point without this"
    );

    let err = play.launch(game.id).unwrap_err().to_string();
    assert!(err.contains("RETROARCH"), "unhelpful message: {err}");

    assert_eq!(play.running_count(), 0);
    assert!(play.db().recent_games(4).unwrap().is_empty());

    std::env::remove_var("RETROARCH");
}
