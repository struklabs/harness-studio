use super::*;
use std::io::Write;

fn is_sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[test]
fn every_spec_pins_its_files_and_archives() {
    for spec in specs() {
        assert!(!spec.archives.is_empty());
        assert!(
            spec.archives
                .iter()
                .flat_map(|a| a.files.iter())
                .any(|f| f.out == spec.lib),
            "{:?} installs no {}",
            spec.variant,
            spec.lib
        );
        for archive in spec.archives {
            assert!(archive.url.starts_with("https://"), "{}", archive.url);
            assert!(is_sha256(archive.sha256), "{}", archive.url);
            for f in archive.files {
                match archive.format {
                    // Whole-archive pin only for tarballs.
                    ArchiveFormat::TarGz => assert!(f.sha256.is_empty()),
                    ArchiveFormat::Zip => assert!(is_sha256(f.sha256), "{}", f.inner),
                }
            }
        }
    }
}

#[test]
fn defaults_and_provider_choices() {
    #[cfg(windows)]
    {
        assert_eq!(default_variant(), OrtVariant::Directml);
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            variant_for_provider_in(tmp.path(), "auto"),
            OrtVariant::Directml
        );
        assert_eq!(variant_for_provider("DML"), OrtVariant::Directml);
    }
    #[cfg(not(windows))]
    assert_eq!(default_variant(), OrtVariant::Cpu);
    assert_eq!(variant_for_provider("cuda"), OrtVariant::Cuda);
    assert_eq!(OrtVariant::parse("DirectML"), Some(OrtVariant::Directml));
    assert_eq!(OrtVariant::parse("tpu"), None);
}

#[test]
fn platform_support_follows_the_pinned_builds() {
    let s = platform_support();
    assert_eq!(s.supported, !specs().is_empty());
    assert_eq!(
        (s.os.as_str(), s.arch.as_str()),
        (std::env::consts::OS, std::env::consts::ARCH)
    );
    // Intel Macs have no official build the sidecar can load (none after 1.23.2).
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    assert!(!s.supported);
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    assert!(s.supported);
}

#[test]
fn a_missing_build_says_whether_the_platform_has_any() {
    // CUDA has no build outside Windows; a platform without any build names itself instead.
    let e = no_spec(OrtVariant::Cuda);
    let (key, params) = crate::error_key::parse(&e).unwrap();
    if supported() {
        assert_eq!(
            (key, params["variant"].as_str()),
            ("onnx.unsupported_platform", Some("cuda"))
        );
    } else {
        assert_eq!(key, "onnx.platform_unsupported");
        assert_eq!(params["arch"], std::env::consts::ARCH);
        assert!(lib_for_provider("auto")
            .unwrap_err()
            .starts_with("onnx.platform_unsupported"));
    }
}

/// A folder laid out the way [`install_in`] leaves it.
fn fake_install(root: &Path, variant: OrtVariant) -> PathBuf {
    let spec = spec_for(variant).expect("variant of this platform");
    let dir = dir_of(root, spec);
    std::fs::create_dir_all(&dir).unwrap();
    let mut files = Vec::new();
    for f in spec.archives.iter().flat_map(|a| a.files.iter()) {
        let bytes = format!("fake {}", f.out).into_bytes();
        std::fs::write(dir.join(f.out), &bytes).unwrap();
        files.push(InstalledFile {
            name: f.out.to_string(),
            size: bytes.len() as u64,
            sha256: hex(&bytes),
        });
    }
    let manifest = Manifest {
        variant,
        version: spec.version.to_string(),
        files,
        sources: vec![],
        installed_at: 1,
    };
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    dir
}

/// Once the CUDA build is installed, `auto` launches with it (and a CPU choice reuses it).
#[test]
fn auto_takes_the_installed_cuda_build() {
    if spec_for(OrtVariant::Cuda).is_none() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    assert_ne!(
        variant_for_provider_in(tmp.path(), "auto"),
        OrtVariant::Cuda
    );
    fake_install(tmp.path(), OrtVariant::Cuda);
    assert_eq!(
        variant_for_provider_in(tmp.path(), "auto"),
        OrtVariant::Cuda
    );
    assert_eq!(
        variant_for_provider_in(tmp.path(), "directml"),
        OrtVariant::Directml
    );
}

/// A CUDA launch never starts the CUDA download on Windows: without the build it fetches (and
/// runs on) DirectML.
#[test]
fn a_cuda_launch_without_the_build_takes_the_default_on_windows() {
    let tmp = tempfile::tempdir().unwrap();
    #[cfg(windows)]
    assert_eq!(launch_variant_in(tmp.path(), "cuda"), OrtVariant::Directml);
    #[cfg(not(windows))]
    assert_eq!(launch_variant_in(tmp.path(), "cuda"), OrtVariant::Cuda);
    if spec_for(OrtVariant::Cuda).is_some() {
        fake_install(tmp.path(), OrtVariant::Cuda);
        assert_eq!(launch_variant_in(tmp.path(), "cuda"), OrtVariant::Cuda);
    }
}

#[test]
fn installed_needs_a_manifest_and_all_files() {
    let tmp = tempfile::tempdir().unwrap();
    let variant = default_variant();
    if spec_for(variant).is_none() {
        return;
    }
    assert!(installed_in(tmp.path(), variant).is_none());
    assert!(!any_installed_in(tmp.path()));
    let dir = fake_install(tmp.path(), variant);
    // The Hub and the library count ONNX Runtime as installed from here.
    assert!(any_installed_in(tmp.path()));
    let got = installed_in(tmp.path(), variant).expect("complete install");
    assert_eq!(got.lib, dir.join(spec_for(variant).unwrap().lib));
    assert!(got.size_bytes > 0);
    let shown = view_in(tmp.path());
    assert!(shown
        .iter()
        .any(|v| v.variant == variant && v.installed.is_some()));

    // A changed size (an interrupted copy, an antivirus quarantine) is "not installed".
    std::fs::write(&got.lib, b"x").unwrap();
    assert!(installed_in(tmp.path(), variant).is_none());
}

/// Windows builds carry the GenAI library; an install from before it was added (its manifest
/// lacks the file) counts as not installed and is fetched again.
#[test]
fn an_install_without_the_genai_library_is_incomplete() {
    let variant = default_variant();
    let Some(spec) = spec_for(variant) else {
        return;
    };
    let has_genai = spec
        .archives
        .iter()
        .flat_map(|a| a.files.iter())
        .any(|f| f.out == GENAI_LIB);
    assert_eq!(has_genai, cfg!(all(windows, target_arch = "x86_64")));
    if !has_genai {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = fake_install(tmp.path(), variant);
    assert!(installed_in(tmp.path(), variant).is_some());
    assert!(dir.join(GENAI_LIB).is_file());
    // Rewrite the manifest the way an older build did: without the GenAI file.
    let mut m = read_manifest(&dir).unwrap();
    m.files.retain(|f| f.name != GENAI_LIB);
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(installed_in(tmp.path(), variant).is_none());
    // Every Windows build ships it (CUDA also its CUDA provider library).
    for s in specs() {
        assert!(
            s.archives
                .iter()
                .flat_map(|a| a.files.iter())
                .any(|f| f.out == GENAI_LIB),
            "{:?}",
            s.variant
        );
    }
    let cuda = spec_for(OrtVariant::Cuda).unwrap();
    assert!(cuda
        .archives
        .iter()
        .flat_map(|a| a.files.iter())
        .any(|f| f.out == "onnxruntime-genai-cuda.dll"));
}

#[test]
fn genai_library_path_follows_the_installed_provider_build() {
    // Nothing is installed in the test data folder.
    assert!(genai_lib_for_provider("directml").is_none());
}

#[test]
fn verify_names_the_files_that_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let variant = default_variant();
    if spec_for(variant).is_none() {
        return;
    }
    let dir = fake_install(tmp.path(), variant);
    assert!(verify_in(tmp.path(), variant).unwrap().is_empty());
    let lib = spec_for(variant).unwrap().lib;
    // Same size, different bytes.
    let mut bytes = std::fs::read(dir.join(lib)).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(dir.join(lib), bytes).unwrap();
    assert_eq!(
        verify_in(tmp.path(), variant).unwrap(),
        vec![lib.to_string()]
    );
    remove_in(tmp.path(), variant).unwrap();
    assert!(!dir.exists());
    assert!(verify_in(tmp.path(), variant).is_err());
}

#[test]
fn tarball_extraction_takes_the_named_members() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("a.tgz");
    {
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&archive).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(gz);
        for (name, body) in [
            ("pkg/lib/real.so.1.2", &b"library"[..]),
            ("pkg/README", b"x"),
        ] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, name, body).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
    }
    let files = [FileSpec {
        inner: "pkg/lib/real.so.1.2",
        out: "real.so",
        sha256: "",
    }];
    let got = extract_tar_gz(&archive, &files).unwrap();
    assert_eq!(got, vec![("real.so".to_string(), b"library".to_vec())]);
    let missing = [FileSpec {
        inner: "pkg/nope",
        out: "nope",
        sha256: "",
    }];
    assert!(extract_tar_gz(&archive, &missing).is_err());
}

/// A server that answers `Range` requests for one in-memory archive (and 200 for a plain GET
/// when `ranges` is off).
async fn serve_archive(bytes: Vec<u8>, ranges: bool) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let bytes = bytes.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let range = head.lines().find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("range: bytes=")
                        .map(str::to_string)
                });
                let total = bytes.len();
                let (status, body, extra) = match (range, ranges) {
                    (Some(r), true) => {
                        let (a, b) = r.trim().split_once('-').unwrap();
                        let (lo, hi) = if a.is_empty() {
                            (total - b.parse::<usize>().unwrap().min(total), total - 1)
                        } else {
                            (
                                a.parse().unwrap(),
                                b.parse::<usize>().unwrap().min(total - 1),
                            )
                        };
                        (
                            "206 Partial Content",
                            bytes[lo..=hi].to_vec(),
                            format!("Content-Range: bytes {}-{}/{}\r\n", lo, hi, total),
                        )
                    }
                    _ => ("200 OK", bytes, String::new()),
                };
                let header = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                    status,
                    body.len(),
                    extra
                );
                let _ = sock.write_all(header.as_bytes()).await;
                let _ = sock.write_all(&body).await;
            });
        }
    });
    format!("http://{}/pkg.zip", addr)
}

fn sample_zip() -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        w.start_file("pkg/lib/onnxruntime.dll", o).unwrap();
        w.write_all(&vec![42u8; 20_000]).unwrap();
        w.start_file("pkg/lib/huge.pdb", o).unwrap();
        // Incompressible, so the archive is much bigger than the 64 KiB tail.
        let mut x = 0x2545_f491u32;
        let noise: Vec<u8> = (0..400_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        w.write_all(&noise).unwrap();
        w.finish().unwrap();
    }
    buf.into_inner()
}

#[tokio::test]
async fn range_requests_fetch_only_the_wanted_entry() {
    let zip = sample_zip();
    let url = serve_archive(zip.clone(), true).await;
    let seen = std::sync::atomic::AtomicU64::new(0);
    let got = remote_zip::fetch_entries(
        &client().unwrap(),
        &url,
        &["pkg/lib/onnxruntime.dll"],
        &|n| seen.store(n, std::sync::atomic::Ordering::SeqCst),
    )
    .await
    .unwrap();
    assert_eq!(got[0].1, vec![42u8; 20_000]);
    // The 400 KB entry (and with it most of the archive) never came over the wire.
    assert!(seen.load(std::sync::atomic::Ordering::SeqCst) < zip.len() as u64);
}

#[tokio::test]
async fn a_server_without_ranges_is_reported() {
    let url = serve_archive(sample_zip(), false).await;
    let err = remote_zip::fetch_entries(
        &client().unwrap(),
        &url,
        &["pkg/lib/onnxruntime.dll"],
        &|_| {},
    )
    .await
    .unwrap_err();
    assert!(err.contains("range"), "{err}");
}

#[tokio::test]
async fn zip_files_are_checked_against_their_pins_and_fall_back_to_the_whole_archive() {
    let zip = sample_zip();
    let good = hex(&vec![42u8; 20_000]);
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let tmp = tempfile::tempdir().unwrap();

    for ranges in [true, false] {
        let url = serve_archive(zip.clone(), ranges).await;
        let files: &'static [FileSpec] = Box::leak(Box::new([FileSpec {
            inner: "pkg/lib/onnxruntime.dll",
            out: "onnxruntime.dll",
            sha256: leak(good.clone()),
        }]));
        let archive = ArchiveSpec {
            url: leak(url.clone()),
            size: zip.len() as u64,
            sha256: leak(hex(&zip)),
            format: ArchiveFormat::Zip,
            files,
        };
        let got = fetch_zip_files(&client().unwrap(), &archive, tmp.path(), 0)
            .await
            .unwrap();
        assert_eq!(got[0].0, "onnxruntime.dll");
        assert_eq!(got[0].1.len(), 20_000, "ranges={ranges}");

        // A wrong pin is refused, whichever way the bytes came.
        let bad: &'static [FileSpec] = Box::leak(Box::new([FileSpec {
            inner: "pkg/lib/onnxruntime.dll",
            out: "onnxruntime.dll",
            sha256: leak("0".repeat(64)),
        }]));
        let tampered = ArchiveSpec {
            files: bad,
            ..archive
        };
        let e = fetch_zip_files(&client().unwrap(), &tampered, tmp.path(), 0)
            .await
            .unwrap_err();
        assert!(e.starts_with("runtime.digest_mismatch"), "{e}");
    }
}

/// Downloads the real DirectML (or default) build into a temp folder, checks the pinned
/// digests and the install state.
///
/// `cargo test -p harness-core --lib real_install -- --ignored --nocapture`
#[tokio::test]
#[ignore = "downloads ONNX Runtime (about 7-20 MB over range requests)"]
async fn real_install_of_the_default_build() {
    let tmp = tempfile::tempdir().unwrap();
    let variant = default_variant();
    let done = install_in(tmp.path(), variant).await.unwrap();
    println!("{:#?}", done);
    assert!(done.lib.is_file());
    assert!(verify_in(tmp.path(), variant).unwrap().is_empty());
    // A second call is a no-op.
    assert_eq!(install_in(tmp.path(), variant).await.unwrap(), done);
}
