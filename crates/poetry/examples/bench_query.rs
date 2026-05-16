use std::path::PathBuf;
use std::time::Instant;

use nsh_poetry::PoetryLibrary;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        eprintln!("usage: cargo run -p nsh-poetry --example bench_query -- <poetry.db> <chars>...");
        std::process::exit(2);
    }

    let db_path = PathBuf::from(args.remove(0));
    let queries = if args.is_empty() {
        vec![
            "风桃樱梨开江春次江第杏晓".to_string(),
            "璃斗江芳琉百红般菲紫月千".to_string(),
        ]
    } else {
        args
    };

    let started = Instant::now();
    let library = PoetryLibrary::open(&db_path)?;
    println!("open_ms={}", started.elapsed().as_millis());

    for round in 1..=5 {
        for query in &queries {
            let started = Instant::now();
            let matches = library.find_poem_from_chars(query, 3)?;
            let elapsed = started.elapsed();
            let answer = matches
                .first()
                .and_then(|item| item.matched_clause.as_deref())
                .unwrap_or("<none>");
            let title = matches
                .first()
                .map(|item| item.poem.title.as_str())
                .unwrap_or("<none>");
            println!(
                "round={round} ms={:.3} answer={answer} title={title} query={query}",
                elapsed.as_secs_f64() * 1000.0
            );
        }
    }

    Ok(())
}
