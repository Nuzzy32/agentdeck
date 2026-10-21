use std::num::NonZeroU16;

fn main() {
    let path = std::env::args().nth(1).expect("usage: agentdeck-s7 <raw log>");
    let bytes = std::fs::read(path).expect("read log");
    // same size as pty_probe.py
    let (rows, cols) = (NonZeroU16::new(40).unwrap(), NonZeroU16::new(120).unwrap());
    let mut parser = vt100::Parser::new(rows, cols, 1000);
    parser.process(&bytes);
    let screen = parser.screen();
    println!("bracketed_paste={} alternate_screen={}", screen.bracketed_paste(), screen.alternate_screen());
    println!("{}", screen.contents());
}
