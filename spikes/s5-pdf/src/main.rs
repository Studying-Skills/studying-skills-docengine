use std::fs;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text = fs::read_to_string(&a[1]).unwrap();
    let dir = &a[2];
    let f = |n: &str| fs::read(format!("{dir}/LiberationSerif-{n}.ttf")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let t = std::time::Instant::now();
    let pdf = s5_pdf::build(&lines, [f("Regular"), f("Bold"), f("Italic")]).unwrap();
    eprintln!("{} lines -> {} bytes in {:?}", lines.len(), pdf.len(), t.elapsed());
    fs::write(&a[3], pdf).unwrap();
}
