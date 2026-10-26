//! Renders the app icon to assets/icon.png (1024x1024).
//! Run with: cargo run --release --example make_icon

use std::f32::consts::TAU;

const SIZE: usize = 1024;
const SS: usize = 4; // supersampling per axis

#[derive(Clone, Copy)]
struct Rgba(f32, f32, f32, f32); // premultiplied

impl Rgba {
    fn rgb(r: u8, g: u8, b: u8, a: f32) -> Self {
        Rgba(r as f32 / 255.0 * a, g as f32 / 255.0 * a, b as f32 / 255.0 * a, a)
    }
    fn over(self, dst: Rgba) -> Rgba {
        let k = 1.0 - self.3;
        Rgba(self.0 + dst.0 * k, self.1 + dst.1 * k, self.2 + dst.2 * k, self.3 + dst.3 * k)
    }
    fn scale(self, a: f32) -> Rgba {
        Rgba(self.0 * a, self.1 * a, self.2 * a, self.3 * a)
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// Apple's icon grid: an 824px body centred in 1024.
const HALF: f32 = 824.0 / 2048.0;

fn squircle(x: f32, y: f32) -> f32 {
    let (dx, dy) = ((x - 0.5).abs() / HALF, (y - 0.5).abs() / HALF);
    (dx.powi(5) + dy.powi(5)).powf(0.2)
}

fn sample(x: f32, y: f32) -> Rgba {
    let s = squircle(x, y);
    let mut out = Rgba(0.0, 0.0, 0.0, 0.0);
    // soft drop shadow
    let sh = squircle(x, y - 0.014);
    out = Rgba(0.0, 0.0, 0.05, 0.38 * smoothstep(1.07, 0.95, sh)).over(out);
    if s > 1.0 {
        return out;
    }

    let top = 0.5 - HALF;
    let t = ((y - top) / (2.0 * HALF)).clamp(0.0, 1.0);
    // dusk gradient: deep indigo -> violet -> sea teal
    let c = if t < 0.55 {
        lerp3([28.0, 24.0, 64.0], [70.0, 62.0, 128.0], t / 0.55)
    } else {
        lerp3([70.0, 62.0, 128.0], [58.0, 132.0, 150.0], (t - 0.55) / 0.45)
    };
    let mut body = Rgba(c[0] / 255.0, c[1] / 255.0, c[2] / 255.0, 1.0);

    // stars
    const STARS: [(f32, f32, f32); 9] = [
        (0.22, 0.20, 0.004),
        (0.31, 0.29, 0.003),
        (0.44, 0.17, 0.0035),
        (0.53, 0.26, 0.0025),
        (0.80, 0.21, 0.003),
        (0.17, 0.34, 0.0025),
        (0.38, 0.38, 0.002),
        (0.84, 0.40, 0.0025),
        (0.27, 0.45, 0.002),
    ];
    for (sx, sy, r) in STARS {
        let d = ((x - sx).powi(2) + (y - sy).powi(2)).sqrt();
        let a = 0.85 * smoothstep(r * 1.6, r * 0.4, d) + 0.18 * (-(d / (r * 5.0)).powi(2)).exp();
        body = Rgba::rgb(255, 248, 230, a.min(1.0)).over(body);
    }

    // moon with glow
    let (mx, my, mr) = (0.665, 0.315, 0.072);
    let d = ((x - mx).powi(2) + (y - my).powi(2)).sqrt();
    let glow = 0.5 * (-((d - mr).max(0.0) / 0.09).powi(2)).exp();
    body = Rgba::rgb(255, 214, 150, glow).over(body);
    let disc = smoothstep(mr + 0.0015, mr - 0.0015, d);
    body = Rgba::rgb(255, 236, 196, disc).over(body);

    // flowing waves (the strings)
    let waves: [(f32, f32, f32, f32, [u8; 3]); 3] = [
        (0.565, 0.040, 1.15, 0.3, [120, 214, 222]),
        (0.590, 0.032, 1.55, 2.1, [196, 168, 244]),
        (0.612, 0.026, 0.95, 4.0, [250, 206, 140]),
    ];
    for (base, amp, freq, phase, col) in waves {
        let wy = base + amp * (TAU * freq * x + phase).sin() * (0.6 + 0.4 * (TAU * 0.5 * x).sin());
        let dist = (y - wy).abs();
        let line = smoothstep(0.0075, 0.0045, dist);
        let halo = 0.30 * (-(dist / 0.022).powi(2)).exp();
        body = Rgba::rgb(col[0], col[1], col[2], (line * 0.95 + halo).min(1.0)).over(body);
    }

    // piano keys along the bottom
    let key_top = 0.70;
    if y > key_top {
        let left = 0.5 - HALF;
        let w = 2.0 * HALF / 7.0;
        let kx = (x - left) / w;
        let i = kx.floor();
        let f = kx - i;
        let gap = smoothstep(0.0, 0.035, f) * smoothstep(1.0, 0.965, f);
        let shade = 0.90 + 0.10 * ((y - key_top) / 0.13).clamp(0.0, 1.0);
        let white = Rgba::rgb((246.0 * shade) as u8, (243.0 * shade) as u8, (236.0 * shade) as u8, 0.96 * gap);
        let mut key = white.over(Rgba::rgb(20, 18, 40, 1.0));
        // black keys sit between white keys 0-1, 1-2, 3-4, 4-5, 5-6
        let black_h = key_top + 0.085;
        let bf = kx - kx.round();
        let between = kx.round() as i32;
        if y < black_h && [1, 2, 4, 5, 6].contains(&between) && bf.abs() < 0.30 {
            let edge = smoothstep(0.30, 0.27, bf.abs()) * smoothstep(black_h, black_h - 0.004, y);
            key = Rgba::rgb(26, 24, 46, edge).over(key);
        }
        // fade in from the scene above
        let fade = smoothstep(key_top, key_top + 0.004, y);
        body = key.scale(fade).over(body.scale(1.0 - fade));
        // a soft shadow line at the top of the keys
        let line = 0.45 * (-((y - key_top) / 0.006).powi(2)).exp();
        body = Rgba(0.0, 0.0, 0.0, line).over(body);
    }

    // subtle top highlight
    let hl = 0.10 * smoothstep(0.35, 0.0, t);
    body = Rgba(hl, hl, hl, 0.0).over(body);

    let cover = smoothstep(1.0, 0.996, s);
    body.scale(cover).over(out)
}

fn main() {
    let mut px = vec![0u8; SIZE * SIZE * 4];
    for py in 0..SIZE {
        for pxi in 0..SIZE {
            let mut acc = Rgba(0.0, 0.0, 0.0, 0.0);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (pxi as f32 + (sx as f32 + 0.5) / SS as f32) / SIZE as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) / SIZE as f32;
                    let c = sample(x, y);
                    acc = Rgba(acc.0 + c.0, acc.1 + c.1, acc.2 + c.2, acc.3 + c.3);
                }
            }
            let n = (SS * SS) as f32;
            let a = acc.3 / n;
            let un = |v: f32| if a > 0.0 { (v / n / a * 255.0).round().clamp(0.0, 255.0) as u8 } else { 0 };
            let i = (py * SIZE + pxi) * 4;
            px[i] = un(acc.0);
            px[i + 1] = un(acc.1);
            px[i + 2] = un(acc.2);
            px[i + 3] = (a * 255.0).round() as u8;
        }
    }
    let file = std::fs::File::create("assets/icon.png").expect("create assets/icon.png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), SIZE as u32, SIZE as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&px).unwrap();
    println!("wrote assets/icon.png");
}
