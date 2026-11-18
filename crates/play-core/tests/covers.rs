//! Covers: the right one is fetched once and kept, a game with none is not
//! asked about again for a while, and no network means nothing is decided.

use play_art::{Fetcher, Sources};
use play_core::art::run_pass;
use play_core::Play;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really an image, but it starts like one";

/// A tiny server that has box art for exactly one game.
fn serve() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            counter.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let mut stream = stream;
            if request.contains(
                "Sega_-_Mega_Drive_-_Genesis/Sonic%20The%20Hedgehog%20%28USA%2C%20Europe%29.png",
            ) {
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    PNG.len()
                )
                .unwrap();
                stream.write_all(PNG).unwrap();
            } else {
                stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .unwrap();
            }
        }
    });
    (format!("http://{addr}"), hits)
}

fn library_with_two_games() -> (tempfile::TempDir, Play) {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("drop");
    std::fs::create_dir_all(&drop).unwrap();
    std::fs::write(
        drop.join("Sonic the Hedgehog (USA, Europe).md"),
        vec![5u8; 512],
    )
    .unwrap();
    std::fs::write(drop.join("Nobody Made This Game (USA).md"), vec![6u8; 512]).unwrap();
    let play = Play::open(&tmp.path().join("library")).unwrap();
    play.intake(&drop, None).unwrap();
    assert_eq!(play.db().game_count().unwrap(), 2);
    (tmp, play)
}

#[test]
fn the_right_cover_is_fetched_once_and_kept() {
    let (_tmp, play) = library_with_two_games();
    let (base, hits) = serve();
    let fetcher = Fetcher::new(Sources::at(&[base.as_str()]));

    let pass = run_pass(play.db(), &play.library, &fetcher);
    assert_eq!(pass.fetched, 1, "{pass:?}");
    assert_eq!(pass.missing, 1, "{pass:?}");
    assert!(pass.offline.is_none());

    let sonic = play.db().list_games("Sonic", None).unwrap().pop().unwrap();
    let path = play.art_path(sonic.id).expect("Sonic has a cover on file");
    assert_eq!(std::fs::read(path).unwrap(), PNG);

    let before = hits.load(Ordering::SeqCst);
    let again = run_pass(play.db(), &play.library, &fetcher);
    assert_eq!(again.fetched, 0);
    assert_eq!(again.skipped, 1, "the miss is remembered: {again:?}");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "nothing was asked twice"
    );
}

#[test]
fn no_network_decides_nothing() {
    let (_tmp, play) = library_with_two_games();
    // A port nothing listens on.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let fetcher = Fetcher::new(Sources::at(&[base.as_str()]));

    let pass = run_pass(play.db(), &play.library, &fetcher);
    assert!(pass.offline.is_some(), "{pass:?}");
    assert_eq!(pass.fetched, 0);

    let (base, _) = serve();
    let fetcher = Fetcher::new(Sources::at(&[base.as_str()]));
    let later = run_pass(play.db(), &play.library, &fetcher);
    assert_eq!(
        later.fetched, 1,
        "offline must not have marked Sonic as coverless"
    );
}

#[test]
fn a_cover_path_outside_the_library_is_never_served() {
    let (_tmp, play) = library_with_two_games();
    let game = play.db().list_games("", None).unwrap().pop().unwrap();
    play.db().set_art(game.id, "/etc/passwd").unwrap();
    assert_eq!(play.art_path(game.id), None);
}
