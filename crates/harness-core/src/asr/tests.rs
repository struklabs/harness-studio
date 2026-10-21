use super::*;

fn touch(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A folder laid out like the archive `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8`.
fn parakeet_dir(root: &Path) -> PathBuf {
    let d = root.join("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8");
    for f in [
        "encoder.int8.onnx",
        "decoder.int8.onnx",
        "joiner.int8.onnx",
        "tokens.txt",
        "test_wavs/en.wav",
    ] {
        touch(&d.join(f), b"x");
    }
    d
}

fn single(root: &Path, name: &str, file: &str) -> PathBuf {
    let d = root.join(name);
    touch(&d.join(file), b"x");
    touch(&d.join("tokens.txt"), b"x");
    d
}

fn vad() -> PathBuf {
    PathBuf::from("vad/silero_vad.onnx")
}

fn params() -> Params {
    Params {
        vad: vad(),
        language: None,
        threads: 3,
    }
}

#[test]
fn a_parakeet_folder_is_a_nemo_transducer() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = parakeet_dir(tmp.path());
    let m = detect(&dir).unwrap();
    assert_eq!(m.family, AsrFamily::NemoTransducer);
    assert_eq!(m.name, "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8");
    assert_eq!(m.quantization, "int8");
    assert_eq!(m.id, dir.join("encoder.int8.onnx").to_string_lossy());
    assert!(m.decoder.is_some() && m.joiner.is_some());
    // The catalog knows this folder's languages.
    assert!(m.languages.iter().any(|l| l == "de"));
}

#[test]
fn single_file_families_are_told_by_their_folder_name() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, family) in [
        (
            "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17",
            AsrFamily::SenseVoice,
        ),
        (
            "sherpa-onnx-paraformer-zh-2024-03-09",
            AsrFamily::Paraformer,
        ),
        (
            "sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-int8",
            AsrFamily::Omnilingual,
        ),
        (
            "sherpa-onnx-dolphin-base-ctc-multi-lang-int8-2025-04-02",
            AsrFamily::Dolphin,
        ),
        (
            "sherpa-onnx-nemo-ctc-en-conformer-medium",
            AsrFamily::NemoCtc,
        ),
    ] {
        let d = single(tmp.path(), name, "model.int8.onnx");
        assert_eq!(detect(&d).unwrap().family, family, "{name}");
    }
}

#[test]
fn zipformer_is_a_plain_transducer_and_int8_wins() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path().join("sherpa-onnx-zipformer-en-2023-06-26");
    for f in [
        "encoder-epoch-99-avg-1.onnx",
        "encoder-epoch-99-avg-1.int8.onnx",
        "decoder-epoch-99-avg-1.onnx",
        "joiner-epoch-99-avg-1.onnx",
        "joiner-epoch-99-avg-1.int8.onnx",
        "tokens.txt",
    ] {
        touch(&d.join(f), b"x");
    }
    let m = detect(&d).unwrap();
    assert_eq!(m.family, AsrFamily::Transducer);
    assert!(m.model_file.ends_with("encoder-epoch-99-avg-1.int8.onnx"));
    assert!(m
        .joiner
        .unwrap()
        .ends_with("joiner-epoch-99-avg-1.int8.onnx"));
    assert!(m.decoder.unwrap().ends_with("decoder-epoch-99-avg-1.onnx"));
}

#[test]
fn what_the_offline_tool_cannot_run_is_not_detected() {
    let tmp = tempfile::tempdir().unwrap();
    // A streaming model that is not a transducer (CTC, Paraformer).
    let d = tmp
        .path()
        .join("sherpa-onnx-streaming-zipformer-ctc-multi-zh-hans-2023-12-13");
    for f in ["ctc-epoch-20-avg-1-chunk-16-left-128.onnx", "tokens.txt"] {
        touch(&d.join(f), b"x");
    }
    assert!(detect(&d).is_none());
    // Canary has no joiner; no tokens, no model.
    let d = tmp
        .path()
        .join("sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8");
    for f in ["encoder.int8.onnx", "decoder.int8.onnx", "tokens.txt"] {
        touch(&d.join(f), b"x");
    }
    assert!(detect(&d).is_none());
    let d = tmp.path().join("sherpa-onnx-paraformer-zh-2024-03-09");
    touch(&d.join("model.onnx"), b"x");
    assert!(detect(&d).is_none(), "no tokens.txt");
    // A text-to-speech folder is not one of these (and the other way round).
    let d = tmp.path().join("vits-piper-tr_TR-fettah-medium-int8");
    for f in ["tr_TR-fettah-medium.onnx", "tokens.txt"] {
        touch(&d.join(f), b"x");
    }
    touch(&d.join("espeak-ng-data/tr_dict"), b"x");
    assert!(detect(&d).is_none());
    assert!(crate::tts::detect(&parakeet_dir(tmp.path())).is_none());
}

#[test]
fn scan_finds_models_in_roots_and_skips_hidden_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    parakeet_dir(&root.join("asr"));
    single(
        &root.join("elsewhere/deep"),
        "sherpa-onnx-paraformer-zh-2024-03-09",
        "model.onnx",
    );
    // The VAD model's folder and the download staging are not scanned.
    parakeet_dir(&root.join("asr/.staging-x"));
    touch(&root.join("asr/.vad/silero_vad.onnx"), b"x");
    let found = scan(std::slice::from_ref(&root));
    let names: Vec<&str> = found.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
            "sherpa-onnx-paraformer-zh-2024-03-09"
        ]
    );
    // The library sees them as speech-recognition files of the sherpa-onnx runtime.
    let library = library_models(std::slice::from_ref(&root));
    assert_eq!(library.len(), 2);
    assert!(library
        .iter()
        .all(|m| m.kind == crate::domain::ModelKind::Asr && is_sherpa_file(&m.path)));
    assert!(library[0]
        .shard_paths
        .iter()
        .any(|p| p.ends_with("tokens.txt")));
}

#[test]
fn deleting_removes_the_folder_inside_a_root_only() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("models");
    let dir = parakeet_dir(&root.join("asr"));
    let outside = tempfile::tempdir().unwrap();
    let other = parakeet_dir(outside.path());
    let roots = vec![root.clone()];
    let e = delete_model_dir(&other.join("encoder.int8.onnx"), &roots).unwrap_err();
    assert!(e.contains("library.outside_library"), "{e}");
    assert!(other.exists());
    let e = delete_model_dir(&root.join("nope/encoder.int8.onnx"), &roots).unwrap_err();
    assert!(e.contains("stt.model_not_found"), "{e}");
    delete_model_dir(&dir.join("encoder.int8.onnx"), &roots).unwrap();
    assert!(!dir.exists());
    assert!(root.join("asr").exists(), "only the model folder goes");
}

#[test]
fn models_are_found_by_name_path_or_unique_part() {
    let tmp = tempfile::tempdir().unwrap();
    parakeet_dir(tmp.path());
    single(
        tmp.path(),
        "sherpa-onnx-paraformer-zh-2024-03-09",
        "model.onnx",
    );
    single(
        tmp.path(),
        "sherpa-onnx-paraformer-en-2024-03-09",
        "model.onnx",
    );
    let models = scan(&[tmp.path().to_path_buf()]);
    assert_eq!(models.len(), 3);
    let name = |w: Option<&str>, exact| find_model(&models, w, exact).map(|m| m.name.clone());
    // Nothing named: the first.
    assert_eq!(name(None, false).unwrap(), models[0].name);
    assert_eq!(
        name(Some("SHERPA-ONNX-PARAFORMER-ZH-2024-03-09"), true).unwrap(),
        "sherpa-onnx-paraformer-zh-2024-03-09"
    );
    assert_eq!(
        name(Some(&models[0].id), true).unwrap(),
        models[0].name,
        "by path"
    );
    assert_eq!(
        name(Some("parakeet"), false).unwrap(),
        "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"
    );
    // The gateway matches whole names only: a part must not take a request from a whisper alias.
    assert!(name(Some("parakeet"), true)
        .unwrap_err()
        .contains("stt.model_not_found"));
    let e = name(Some("paraformer"), false).unwrap_err();
    assert!(
        e.contains("stt.model_ambiguous") && e.contains("paraformer-en"),
        "{e}"
    );
    assert!(name(Some("whisper-1"), false)
        .unwrap_err()
        .contains("stt.model_not_found"));
    assert!(find_model(&[], None, false)
        .unwrap_err()
        .contains("stt.no_model"));
}

fn model(family: AsrFamily, name: &str) -> AsrModel {
    let dir = PathBuf::from("m").join(name);
    AsrModel {
        id: "x".into(),
        name: name.into(),
        family,
        model_file: dir.join("model.int8.onnx"),
        decoder: Some(dir.join("decoder.int8.onnx")),
        joiner: Some(dir.join("joiner.int8.onnx")),
        preprocessor: None,
        tokens: dir.join("tokens.txt"),
        dir,
        size_bytes: 1,
        quantization: "int8".into(),
        languages: vec![],
    }
}

fn has(args: &[String], wanted: &str) -> bool {
    args.iter().any(|a| a == wanted)
}

fn arg_path(args: &[String], flag: &str) -> String {
    args.iter()
        .find_map(|a| a.strip_prefix(&format!("{flag}=")))
        .unwrap_or_else(|| panic!("{flag} in {args:?}"))
        .replace('\\', "/")
}

#[test]
fn transducer_arguments() {
    let m = model(AsrFamily::NemoTransducer, "p");
    let a = build_args(&m, &params());
    assert_eq!(arg_path(&a, "--encoder"), "m/p/model.int8.onnx");
    assert_eq!(arg_path(&a, "--decoder"), "m/p/decoder.int8.onnx");
    assert_eq!(arg_path(&a, "--joiner"), "m/p/joiner.int8.onnx");
    assert_eq!(arg_path(&a, "--tokens"), "m/p/tokens.txt");
    assert_eq!(arg_path(&a, "--silero-vad-model"), "vad/silero_vad.onnx");
    assert!(has(&a, "--model-type=nemo_transducer"));
    assert!(has(&a, "--silero-vad-max-speech-duration=25"));
    assert!(has(&a, "--num-threads=3"));
    assert!(has(&a, "--print-args=false"));
    let a = build_args(&model(AsrFamily::Transducer, "z"), &params());
    assert!(has(&a, "--model-type=transducer"));
}

#[test]
fn single_model_arguments() {
    let p = params();
    let a = build_args(&model(AsrFamily::Omnilingual, "o"), &p);
    assert_eq!(
        arg_path(&a, "--omnilingual-asr-model"),
        "m/o/model.int8.onnx"
    );
    assert!(!a.iter().any(|x| x.starts_with("--encoder")));
    let a = build_args(&model(AsrFamily::Paraformer, "p"), &p);
    assert_eq!(arg_path(&a, "--paraformer"), "m/p/model.int8.onnx");
    assert!(has(&a, "--model-type=paraformer"));
    let a = build_args(&model(AsrFamily::NemoCtc, "c"), &p);
    assert_eq!(arg_path(&a, "--nemo-ctc-model"), "m/c/model.int8.onnx");
    assert!(has(&a, "--model-type=nemo_ctc"));
    let a = build_args(&model(AsrFamily::Dolphin, "d"), &p);
    assert_eq!(arg_path(&a, "--dolphin-model"), "m/d/model.int8.onnx");
    // Threads never go below one.
    let a = build_args(&model(AsrFamily::Dolphin, "d"), &Params { threads: 0, ..p });
    assert!(has(&a, "--num-threads=1"));
}

#[test]
fn sense_voice_takes_the_requested_language() {
    let m = model(AsrFamily::SenseVoice, "s");
    let a = build_args(&m, &params());
    assert!(has(&a, "--sense-voice-language=auto"));
    assert!(has(&a, "--sense-voice-use-itn=true"));
    assert_eq!(arg_path(&a, "--sense-voice-model"), "m/s/model.int8.onnx");
    for (given, wanted) in [
        (Some("ja"), "ja"),
        (Some("Japanese"), "ja"),
        (Some("zh-CN"), "zh"),
        (Some("EN"), "en"),
        (Some("yue"), "yue"),
        (Some("ko_KR"), "ko"),
        (Some("tr"), "auto"),
        (Some(""), "auto"),
        (None, "auto"),
    ] {
        assert_eq!(sense_voice_language(given), wanted, "{given:?}");
    }
}

#[test]
fn the_tools_lines_become_segments_moved_back_by_the_padding() {
    let out = "Started!\n0.550 -- 3.020: The quick brown fox.\n  3.900 -- 10.188: Second one, with: a colon.\n11.000 -- 11.500: \nElapsed seconds: 0.3 s\n";
    let s = parse_output(out, 0.5, 10.0);
    assert_eq!(s.len(), 2);
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(near(s[0].start, 0.05) && near(s[0].end, 2.52), "{:?}", s[0]);
    assert_eq!(s[0].text, "The quick brown fox.");
    assert!(near(s[1].start, 3.4) && near(s[1].end, 9.688), "{:?}", s[1]);
    assert_eq!(s[1].text, "Second one, with: a colon.");
    // Times stay inside the audio: padding at the end, speech in the first padding.
    let s = parse_output("0.100 -- 0.900: early\n10.400 -- 11.200: late\n", 0.5, 10.0);
    assert_eq!((s[0].start, s[0].end), (0.0, 0.4));
    assert!(near(s[1].start, 9.9) && near(s[1].end, 10.0));
    assert!(parse_output("", 0.5, 10.0).is_empty());
    assert!(parse_output("garbage in -- garbage: out", 0.5, 10.0).is_empty());
}

fn transcript() -> Transcript {
    Transcript::new(
        vec![
            Segment {
                start: 0.05,
                end: 2.5,
                text: "Hello there.".into(),
                speaker: None,
            },
            Segment {
                start: 3725.0049,
                end: 3727.5,
                text: "Second line.".into(),
                speaker: None,
            },
        ],
        3730.0,
        Some("en".into()),
        "m".into(),
        1200,
    )
}

#[test]
fn a_transcript_is_shaped_like_openais() {
    let t = transcript();
    assert_eq!(t.text, "Hello there. Second line.");
    assert_eq!(
        t.to_json(),
        serde_json::json!({"text": "Hello there. Second line."})
    );
    let v = t.to_verbose_json("transcribe");
    assert_eq!(v["task"], "transcribe");
    assert_eq!(v["language"], "en");
    assert_eq!(v["duration"], 3730.0);
    assert_eq!(v["segments"].as_array().unwrap().len(), 2);
    assert_eq!(v["segments"][1]["id"], 1);
    assert_eq!(v["segments"][1]["start"], 3725.005);
    assert_eq!(v["segments"][0]["text"], "Hello there.");
    // No language asked: the field is left out.
    let mut t = transcript();
    t.language = None;
    assert!(t.to_verbose_json("transcribe").get("language").is_none());
}

#[test]
fn subtitles() {
    let t = transcript();
    assert_eq!(
        t.to_srt(),
        "1\n00:00:00,050 --> 00:00:02,500\nHello there.\n\n2\n01:02:05,005 --> 01:02:07,500\nSecond line.\n\n"
    );
    assert_eq!(
        t.to_vtt(),
        "WEBVTT\n\n00:00:00.050 --> 00:00:02.500\nHello there.\n\n01:02:05.005 --> 01:02:07.500\nSecond line.\n\n"
    );
    assert_eq!(format_time(59.9996, ','), "00:01:00,000");
    assert_eq!(format_time(-3.0, '.'), "00:00:00.000");
}

#[test]
fn cjk_segments_join_without_spaces() {
    let seg = |t: &str| Segment {
        start: 0.0,
        end: 1.0,
        text: t.into(),
        speaker: None,
    };
    assert_eq!(join_segments(&[seg("你好，"), seg("世界")]), "你好，世界");
    assert_eq!(
        join_segments(&[seg("こんにちは"), seg("世界")]),
        "こんにちは世界"
    );
    assert_eq!(join_segments(&[seg("hello"), seg("世界")]), "hello 世界");
    assert_eq!(join_segments(&[seg("a"), seg("  "), seg("b")]), "a b");
    assert_eq!(join_segments(&[]), "");
}

#[test]
fn streaming_transducers_and_cohere_transcribe_are_detected() {
    let tmp = tempfile::tempdir().unwrap();
    for name in [
        "sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11",
        "sherpa-onnx-streaming-zipformer-en-2023-06-26",
    ] {
        let d = tmp.path().join(name);
        for f in [
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ] {
            touch(&d.join(f), b"x");
        }
        let m = detect(&d).unwrap();
        assert_eq!(m.family, AsrFamily::StreamingTransducer, "{name}");
        assert!(m.family.is_streaming());
        // The online decoder: no VAD, no `--model-type`, one result for the whole file.
        let args = build_args(&m, &params());
        assert!(args.iter().any(|a| a.starts_with("--joiner=")));
        assert!(args.contains(&"--enable-endpoint=false".to_string()));
        assert!(!args
            .iter()
            .any(|a| a.starts_with("--model-type") || a.contains("vad")));
    }
    let d = tmp
        .path()
        .join("sherpa-onnx-cohere-transcribe-14-lang-int8-2026-04-01");
    for f in [
        "encoder.int8.onnx",
        "encoder.int8.onnx.data",
        "decoder.int8.onnx",
        "tokens.txt",
    ] {
        touch(&d.join(f), b"x");
    }
    let m = detect(&d).unwrap();
    assert_eq!(m.family, AsrFamily::CohereTranscribe);
    assert!(!m.family.is_streaming());
    let mut p = params();
    p.language = Some("de-DE".into());
    let args = build_args(&m, &p);
    for want in [
        "--cohere-transcribe-language=de",
        "--cohere-transcribe-use-punct=true",
    ] {
        assert!(args.iter().any(|a| a == want), "{want}: {args:?}");
    }
    assert!(
        args.iter()
            .any(|a| a.starts_with("--cohere-transcribe-decoder=")
                && a.ends_with("decoder.int8.onnx"))
    );
}

#[test]
fn cohere_takes_its_fourteen_languages_and_english_by_default() {
    assert_eq!(cohere_language(None), Some("en"));
    assert_eq!(cohere_language(Some("  ")), Some("en"));
    assert_eq!(cohere_language(Some("ja-JP")), Some("ja"));
    assert_eq!(cohere_language(Some("German")), Some("de"));
    assert_eq!(cohere_language(Some("tr")), None);
}

#[test]
fn the_online_decoder_result_is_cut_into_segments() {
    // What `sherpa-onnx` wrote for two voices with a pause between them (0.5 s of padding in
    // front), trimmed: tokens with their start times.
    let stderr = r#"Number of threads: 4
{ "text": "Good morning.  How was it?  It was wonderful", "tokens": [" Good", " ", "mo", "r", "ni", "ng", ".", " ", " How", " was", " it", "?", " ", " ", "I", "t", " was", " ", "w", "ond", "er", "ful"], "timestamps": [0.64, 0.80, 0.80, 0.80, 0.96, 0.96, 1.20, 1.20, 1.36, 1.60, 1.76, 3.40, 3.40, 3.40, 3.60, 3.60, 3.84, 4.08, 4.08, 4.16, 4.24, 4.30], "segment": 0}
"#;
    let segs = parse_online_json(stderr, 0.5, 10.0);
    let texts: Vec<&str> = segs.iter().map(|s| s.text.as_str()).collect();
    // "morning." is followed by a short pause (0.4 s between the words): one segment. "it?" is
    // emitted late, just before the next word, but the words around it are 1.84 s apart: cut.
    assert_eq!(texts, ["Good morning. How was it?", "It was wonderful"]);
    assert!((segs[0].start - 0.14).abs() < 1e-9);
    assert!((segs[0].end - 1.51).abs() < 1e-9);
    assert!((segs[1].start - 3.1).abs() < 1e-9);
    assert!((segs[1].end - 4.05).abs() < 1e-9);
    // No JSON: nothing.
    assert!(parse_online_json("Elapsed seconds: 1", 0.5, 10.0).is_empty());
}

fn make(dir: &std::path::Path, files: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    for f in files {
        std::fs::write(dir.join(f), b"x").unwrap();
    }
}

#[test]
fn whisper_and_moonshine_folders_are_detected_and_get_their_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let whisper = tmp.path().join("sherpa-onnx-whisper-tiny.en");
    make(
        &whisper,
        &[
            "tiny.en-decoder.int8.onnx",
            "tiny.en-decoder.onnx",
            "tiny.en-encoder.int8.onnx",
            "tiny.en-encoder.onnx",
            "tiny.en-tokens.txt",
        ],
    );
    let m = detect(&whisper).unwrap();
    assert_eq!(m.family, AsrFamily::Whisper);
    assert!(m.model_file.ends_with("tiny.en-encoder.int8.onnx"));
    assert!(m.tokens.ends_with("tiny.en-tokens.txt"));
    assert_eq!(m.quantization, "int8");
    let a = build_args(&m, &params());
    assert!(arg_path(&a, "--whisper-encoder").ends_with("tiny.en-encoder.int8.onnx"));
    assert!(arg_path(&a, "--whisper-decoder").ends_with("tiny.en-decoder.int8.onnx"));
    assert!(!a.iter().any(|x| x.starts_with("--whisper-tail-paddings")));
    assert!(!a.iter().any(|x| x.starts_with("--whisper-language")));

    // A multilingual model takes the request's language.
    let multi = tmp.path().join("sherpa-onnx-whisper-base");
    make(
        &multi,
        &["base-encoder.onnx", "base-decoder.onnx", "base-tokens.txt"],
    );
    let m = detect(&multi).unwrap();
    let p = Params {
        language: Some("tr-TR".into()),
        ..params()
    };
    let a = build_args(&m, &p);
    assert!(has(&a, "--whisper-language=tr"));

    let moon = tmp.path().join("sherpa-onnx-moonshine-tiny-en-int8");
    make(
        &moon,
        &[
            "cached_decode.int8.onnx",
            "encode.int8.onnx",
            "preprocess.onnx",
            "uncached_decode.int8.onnx",
            "tokens.txt",
        ],
    );
    let m = detect(&moon).unwrap();
    assert_eq!(m.family, AsrFamily::Moonshine);
    let a = build_args(&m, &params());
    assert!(arg_path(&a, "--moonshine-preprocessor").ends_with("preprocess.onnx"));
    assert!(arg_path(&a, "--moonshine-encoder").ends_with("encode.int8.onnx"));
    assert!(arg_path(&a, "--moonshine-uncached-decoder").ends_with("uncached_decode.int8.onnx"));
    assert!(arg_path(&a, "--moonshine-cached-decoder").ends_with("cached_decode.int8.onnx"));
    assert!(a.iter().any(|x| x.starts_with("--silero-vad-model=")));

    // An Optimum export of Whisper is no sherpa folder.
    let optimum = tmp.path().join("whisper-tiny.en");
    make(
        &optimum,
        &["encoder_model.onnx", "decoder_model.onnx", "tokens.txt"],
    );
    assert!(detect(&optimum).is_none());
}

#[test]
fn whisper_language_codes() {
    assert_eq!(whisper_language(None), None);
    assert_eq!(whisper_language(Some("auto")), None);
    assert_eq!(whisper_language(Some("")), None);
    assert_eq!(whisper_language(Some("tr")).as_deref(), Some("tr"));
    assert_eq!(whisper_language(Some("Turkish")).as_deref(), Some("tr"));
    assert_eq!(whisper_language(Some("de_DE")).as_deref(), Some("de"));
    assert_eq!(whisper_language(Some("zh;rm -rf")), None);
}
