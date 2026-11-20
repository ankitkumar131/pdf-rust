//! Shared test helpers: a minimal PDF writer (including RC4 encryption) and
//! small filesystem utilities. Keeping the fixture generator in-tree means the
//! test suite has no binary assets and no network dependencies.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// One page of a generated test document.
#[derive(Clone)]
pub struct TestPage {
    pub width: f32,
    pub height: f32,
    pub rotate: u16,
    pub text_lines: Vec<String>,
    pub font_size: f32,
    /// Link annotations: (x1, y1, x2, y2, uri) in PDF points.
    pub links: Vec<(f32, f32, f32, f32, String)>,
}

impl TestPage {
    pub fn a4(rotate: u16) -> TestPage {
        TestPage {
            width: 595.0,
            height: 842.0,
            rotate,
            text_lines: Vec::new(),
            font_size: 24.0,
            links: Vec::new(),
        }
    }

    pub fn with_text(mut self, lines: &[&str]) -> TestPage {
        self.text_lines = lines.iter().map(|s| s.to_string()).collect();
        self
    }
}

/// Builds a real, spec-conforming PDF file byte by byte.
pub struct PdfBuilder {
    pub pages: Vec<TestPage>,
    pub outline: Vec<(String, u32)>,
    pub title: Option<String>,
    pub author: Option<String>,
    /// (user password, owner password) — RC4 40-bit standard security handler.
    pub encrypt: Option<(String, String)>,
}

impl Default for PdfBuilder {
    fn default() -> Self {
        PdfBuilder {
            pages: vec![TestPage::a4(0).with_text(&["Test page"])],
            outline: Vec::new(),
            title: None,
            author: None,
            encrypt: None,
        }
    }
}

impl PdfBuilder {
    pub fn new(pages: Vec<TestPage>) -> PdfBuilder {
        PdfBuilder {
            pages,
            ..Default::default()
        }
    }

    /// Convenience: `count` pages, each labelled "Page N".
    pub fn with_pages(count: u32) -> PdfBuilder {
        let pages = (1..=count)
            .map(|i| {
                let mut page = TestPage::a4(0);
                page.text_lines = vec![format!("Page {}", i), format!("Body of page {}", i)];
                page
            })
            .collect();
        PdfBuilder::new(pages)
    }

    pub fn build(&self) -> Vec<u8> {
        let encryptor = self
            .encrypt
            .as_ref()
            .map(|(user, owner)| crypto::StandardSecurity::new(user, owner));
        let mut objects: Vec<Vec<u8>> = Vec::new();
        // Object numbers are assigned in push order, starting at 1.
        let mut push = |objects: &mut Vec<Vec<u8>>, body: Vec<u8>| -> usize {
            objects.push(body);
            objects.len()
        };

        push(&mut objects, Vec::new()); // 1 catalog
        push(&mut objects, Vec::new()); // 2 page tree
        push(&mut objects, Vec::new()); // 3 outlines
        let font_obj = push(
            &mut objects,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        );

        let mut page_objs = Vec::new();
        for page in &self.pages {
            let mut content = String::from("BT\n");
            content.push_str(&format!(
                "/F1 {} Tf\n72 {} Td\n20 TL\n",
                page.font_size,
                page.height - 80.0
            ));
            for line in &page.text_lines {
                content.push_str(&format!("({}) Tj T*\n", escape_pdf_string(line)));
            }
            content.push_str("ET\n");
            let raw = content.into_bytes();
            let content_obj_number = objects.len() + 1;
            let stream = match &encryptor {
                Some(sec) => sec.encrypt_object(content_obj_number, &raw),
                None => raw,
            };
            let mut body = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
            body.extend_from_slice(&stream);
            body.extend_from_slice(b"\nendstream");
            let content_obj = push(&mut objects, body);

            let mut page_dict = format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 {} 0 R >> >> /Contents {} 0 R",
                page.width, page.height, font_obj, content_obj
            );
            if page.rotate != 0 {
                page_dict.push_str(&format!(" /Rotate {}", page.rotate));
            }
            if !page.links.is_empty() {
                let mut annots = String::from(" /Annots [");
                for (x1, y1, x2, y2, uri) in &page.links {
                    let annot = push(
                        &mut objects,
                        format!(
                            "<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] /Border [0 0 0] /A << /S /URI /URI ({}) >> >>",
                            x1,
                            y1,
                            x2,
                            y2,
                            escape_pdf_string(uri)
                        )
                        .into_bytes(),
                    );
                    annots.push_str(&format!("{} 0 R ", annot));
                }
                annots.push(']');
                page_dict.push_str(&annots);
            }
            page_dict.push_str(" >>");
            page_objs.push(push(&mut objects, page_dict.into_bytes()));
        }

        let kids: Vec<String> = page_objs.iter().map(|n| format!("{} 0 R", n)).collect();
        objects[1] = format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            page_objs.len(),
            kids.join(" ")
        )
        .into_bytes();

        let mut catalog = String::from("<< /Type /Catalog /Pages 2 0 R");
        if !self.outline.is_empty() {
            catalog.push_str(" /Outlines 3 0 R");
        }
        catalog.push_str(" >>");
        objects[0] = catalog.into_bytes();

        if !self.outline.is_empty() {
            let mut items = Vec::new();
            for (title, page_number) in &self.outline {
                let target = page_objs
                    .get((*page_number as usize).saturating_sub(1))
                    .copied()
                    .unwrap_or(page_objs[0]);
                let item = push(
                    &mut objects,
                    format!(
                        "<< /Title ({}) /Parent 3 0 R /Dest [{} 0 R /Fit] >>",
                        escape_pdf_string(title),
                        target
                    )
                    .into_bytes(),
                );
                items.push((item, target, title.clone()));
            }
            let first = items.first().map(|i| i.0).unwrap_or(0);
            let last = items.last().map(|i| i.0).unwrap_or(0);
            objects[2] = format!(
                "<< /Type /Outlines /First {} 0 R /Last {} 0 R /Count {} >>",
                first,
                last,
                items.len()
            )
            .into_bytes();
            for (i, (item, target, title)) in items.iter().enumerate() {
                let prev = if i > 0 { format!("/Prev {} 0 R ", items[i - 1].0) } else { String::new() };
                let next = if i + 1 < items.len() { format!("/Next {} 0 R ", items[i + 1].0) } else { String::new() };
                objects[item - 1] = format!(
                    "<< /Title ({}) /Parent 3 0 R {}{}/Dest [{} 0 R /Fit] >>",
                    escape_pdf_string(title),
                    prev,
                    next,
                    target
                )
                .into_bytes();
            }
        }

        let mut info = String::from("<<");
        if let Some(t) = &self.title {
            info.push_str(&format!(" /Title ({})", escape_pdf_string(t)));
        }
        if let Some(a) = &self.author {
            info.push_str(&format!(" /Author ({})", escape_pdf_string(a)));
        }
        info.push_str(" /Producer (pdf-rust test suite) >>");
        let info_obj = push(&mut objects, info.into_bytes());

        let mut encrypt_obj = 0;
        if let Some(sec) = &encryptor {
            encrypt_obj = push(
                &mut objects,
                format!(
                    "<< /Filter /Standard /V 1 /R 2 /O <{}> /U <{}> /P {} /Length 40 >>",
                    hex(&sec.o),
                    hex(&sec.u),
                    -44
                )
                .into_bytes(),
            );
        }

        // Serialise with an xref table.
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::with_capacity(objects.len());
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_pos = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for off in &offsets {
            out.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
        }
        let mut trailer = format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info {} 0 R",
            objects.len() + 1,
            info_obj
        );
        if encrypt_obj > 0 {
            let id = encryptor.as_ref().map(|s| hex(&s.file_id)).unwrap_or_default();
            trailer.push_str(&format!(" /Encrypt {} 0 R /ID [<{}> <{}>]", encrypt_obj, id, id));
        }
        trailer.push_str(&format!(" >>\nstartxref\n{}\n%%EOF\n", xref_pos));
        out.extend_from_slice(trailer.as_bytes());
        out
    }

    pub fn write_to(&self, path: &Path) -> PathBuf {
        let bytes = self.build();
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).unwrap();
        std::fs::write(path, bytes).unwrap();
        path.to_path_buf()
    }
}

fn escape_pdf_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 128 => out.push(c),
            // Non-ASCII is replaced: the tests only use the standard 14 fonts.
            _ => out.push('?'),
        }
    }
    out
}

pub fn hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        s.push_str(&format!("{:02X}", b));
    }
    s
}

/// Standard security handler (V1/R2, RC4 40-bit) — just enough to create
/// password-protected fixtures.
pub mod crypto {
    const PAD: [u8; 32] = [
        0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01,
        0x08, 0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53,
        0x69, 0x7A,
    ];

    pub struct StandardSecurity {
        pub o: Vec<u8>,
        pub u: Vec<u8>,
        pub key: Vec<u8>,
        pub file_id: Vec<u8>,
    }

    fn pad_password(password: &str) -> [u8; 32] {
        let bytes = password.as_bytes();
        let mut out = [0u8; 32];
        let n = bytes.len().min(32);
        out[..n].copy_from_slice(&bytes[..n]);
        if n < 32 {
            out[n..].copy_from_slice(&PAD[..32 - n]);
        }
        out
    }

    impl StandardSecurity {
        pub fn new(user: &str, owner: &str) -> StandardSecurity {
            let owner_key = md5(&pad_password(owner))[..5].to_vec();
            let o = rc4(&owner_key, &pad_password(user)).to_vec();
            let permissions: i32 = -44;
            let mut input = Vec::new();
            input.extend_from_slice(&pad_password(user));
            input.extend_from_slice(&o);
            input.extend_from_slice(&permissions.to_le_bytes());
            let file_id = md5(format!("pdf-rust-{}", user).as_bytes())[..16].to_vec();
            input.extend_from_slice(&file_id[..16]);
            let key = md5(&input)[..5].to_vec();
            // PDFium (and other readers) verify R2 user passwords against the
            // *default pass-code* rather than the padded password, which is
            // identical for an empty user password and matches what real-world
            // writers emit. Using it here keeps the fixtures readable by PDFium.
            let u = rc4(&key, &PAD).to_vec();
            StandardSecurity {
                o,
                u,
                key,
                file_id,
            }
        }

        /// Encrypts an object's stream with the per-object RC4 key.
        pub fn encrypt_object(&self, object_number: usize, data: &[u8]) -> Vec<u8> {
            let mut input = self.key.clone();
            let n = object_number as u32;
            input.extend_from_slice(&n.to_le_bytes()[..3]);
            input.extend_from_slice(&0u16.to_le_bytes());
            let obj_key = md5(&input)[..10.min(16)].to_vec();
            rc4(&obj_key, data)
        }
    }

    pub fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
        let mut s: [u8; 256] = [0; 256];
        for (i, v) in s.iter_mut().enumerate() {
            *v = i as u8;
        }
        let mut j = 0u8;
        for i in 0..256 {
            j = j
                .wrapping_add(s[i])
                .wrapping_add(key[i % key.len().max(1)]);
            s.swap(i, j as usize);
        }
        let mut out = Vec::with_capacity(data.len());
        let (mut i, mut j) = (0u8, 0u8);
        for &byte in data {
            i = i.wrapping_add(1);
            j = j.wrapping_add(s[i as usize]);
            s.swap(i as usize, j as usize);
            let k = s[(s[i as usize].wrapping_add(s[j as usize])) as usize];
            out.push(byte ^ k);
        }
        out
    }

    pub fn md5(data: &[u8]) -> [u8; 16] {
        const S: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20,
            5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
            6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];
        const K: [u32; 64] = [
            0xd76a_a478, 0xe8c7_b756, 0x2420_70db, 0xc1bd_ceee, 0xf57c_0faf, 0x4787_c62a,
            0xa830_4613, 0xfd46_9501, 0x6980_98d8, 0x8b44_f7af, 0xffff_5bb1, 0x895c_d7be,
            0x6b90_1122, 0xfd98_7193, 0xa679_438e, 0x49b4_0821, 0xf61e_2562, 0xc040_b340,
            0x265e_5a51, 0xe9b6_c7aa, 0xd62f_105d, 0x0244_1453, 0xd8a1_e681, 0xe7d3_fbc8,
            0x21e1_cde6, 0xc337_07d6, 0xf4d5_0d87, 0x455a_14ed, 0xa9e3_e905, 0xfcef_a3f8,
            0x676f_02d9, 0x8d2a_4c8a, 0xfffa_3942, 0x8771_f681, 0x6d9d_6122, 0xfde5_380c,
            0xa4be_ea44, 0x4bde_cfa9, 0xf6bb_4b60, 0xbebf_bc70, 0x289b_7ec6, 0xeaa1_27fa,
            0xd4ef_3085, 0x0488_1d05, 0xd9d4_d039, 0xe6db_99e5, 0x1fa2_7cf8, 0xc4ac_5665,
            0xf429_2244, 0x432a_ff97, 0xab94_23a7, 0xfc93_a039, 0x655b_59c3, 0x8f0c_cc92,
            0xffef_f47d, 0x8584_5dd1, 0x6fa8_7e4f, 0xfe2c_e6e0, 0xa301_4314, 0x4e08_11a1,
            0xf753_7e82, 0xbd3a_f235, 0x2ad7_d2bb, 0xeb86_d391,
        ];
        let mut a0: u32 = 0x6745_2301;
        let mut b0: u32 = 0xefcd_ab89;
        let mut c0: u32 = 0x98ba_dcfe;
        let mut d0: u32 = 0x1032_5476;
        let mut msg = data.to_vec();
        let bit_len = (data.len() as u64).wrapping_mul(8);
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bit_len.to_le_bytes());
        for chunk in msg.chunks(64) {
            let mut m = [0u32; 16];
            for i in 0..16 {
                m[i] = u32::from_le_bytes([
                    chunk[i * 4],
                    chunk[i * 4 + 1],
                    chunk[i * 4 + 2],
                    chunk[i * 4 + 3],
                ]);
            }
            let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
            for i in 0..64 {
                let (f, g) = match i / 16 {
                    0 => ((b & c) | (!b & d), i),
                    1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                    2 => (b ^ c ^ d, (3 * i + 5) % 16),
                    _ => (c ^ (b | !d), (7 * i) % 16),
                };
                let f = f
                    .wrapping_add(a)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g]);
                a = d;
                d = c;
                c = b;
                b = b.wrapping_add(f.rotate_left(S[i]));
            }
            a0 = a0.wrapping_add(a);
            b0 = b0.wrapping_add(b);
            c0 = c0.wrapping_add(c);
            d0 = d0.wrapping_add(d);
        }
        let mut out = [0u8; 16];
        out[..4].copy_from_slice(&a0.to_le_bytes());
        out[4..8].copy_from_slice(&b0.to_le_bytes());
        out[8..12].copy_from_slice(&c0.to_le_bytes());
        out[12..].copy_from_slice(&d0.to_le_bytes());
        out
    }
}

/// Creates a unique temporary directory for a test.
pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        let unique = format!(
            "pdfrust-test-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// True when the PDF engine is available in this environment.
pub fn pdfium_available() -> bool {
    pdf_rust::pdf::ffi::available()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_pdf_has_valid_structure() {
        let bytes = PdfBuilder::with_pages(3).build();
        assert!(bytes.starts_with(b"%PDF-1.7"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("/Count 3"));
        assert!(text.contains("startxref"));
    }

    #[test]
    fn md5_and_rc4_match_known_vectors() {
        assert_eq!(hex(&crypto::md5(b"abc")), "900150983CD24FB0D6963F7D28E17F72");
        assert_eq!(hex(&crypto::rc4(b"Key", b"Plaintext")), "BBF316E8D940AF0AD3");
    }
}
