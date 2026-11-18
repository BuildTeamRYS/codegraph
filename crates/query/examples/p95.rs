use codegraph_query as q;
use codegraph_store::Store;
use std::time::Instant;

fn p95(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[(v.len() as f64 * 0.95) as usize]
}

fn main() {
    let db = std::env::args().nth(1).unwrap();
    let store = Store::open(std::path::Path::new(&db)).unwrap();
    let fresh = q::Freshness::Fresh("benchcommit".into());

    let mut t = Vec::with_capacity(300);
    for _ in 0..300 {
        let s = Instant::now();
        let _ = q::find_symbol(&store, &fresh, "helper", Some(800)).unwrap();
        t.push(s.elapsed().as_micros() as f64 / 1000.0);
    }
    println!("find_symbol p95: {:.2}ms", p95(t));

    let mut t = Vec::with_capacity(300);
    for i in 0..300 {
        let s = Instant::now();
        let _ = q::outline(&store, &fresh, &format!("src/mod{}.ts", i % 550), Some(800)).unwrap();
        t.push(s.elapsed().as_micros() as f64 / 1000.0);
    }
    println!("outline p95: {:.2}ms", p95(t));

    let mut t = Vec::with_capacity(300);
    for i in 0..300 {
        let s = Instant::now();
        let _ = q::get_context(&store, &fresh, &format!("helper{}", i % 400), Some(800)).unwrap();
        t.push(s.elapsed().as_micros() as f64 / 1000.0);
    }
    println!("get_context p95: {:.2}ms", p95(t));

    let mut t = Vec::with_capacity(300);
    for i in 0..300 {
        let s = Instant::now();
        let _ = q::callees(
            &store,
            &fresh,
            &format!("Service{}", 100 + i % 300),
            Some(800),
        )
        .unwrap();
        t.push(s.elapsed().as_micros() as f64 / 1000.0);
    }
    println!("callees p95: {:.2}ms", p95(t));
}
