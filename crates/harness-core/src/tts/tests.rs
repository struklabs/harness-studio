use super::*;

fn touch(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A folder laid out like `kokoro-int8-multi-lang-v1_0` (the real archive's file names).
fn kokoro_dir(root: &Path, name: &str) -> PathBuf {
    let d = root.join(name);
    for f in [
        "model.int8.onnx",
        "voices.bin",
        "tokens.txt",
        "lexicon-us-en.txt",
        "lexicon-gb-en.txt",
        "lexicon-zh.txt",
        "phone-zh.fst",
        "date-zh.fst",
        "number-zh.fst",
        "README.md",
    ] {
        touch(&d.join(f), b"x");
    }
    touch(&d.join("espeak-ng-data/en_dict"), b"x");
    d
}

/// A folder laid out like `vits-piper-tr_TR-fettah-medium-int8`, with the voice's JSON.
fn piper_dir(root: &Path, name: &str, file: &str, json: &str) -> PathBuf {
    let d = root.join(name);
    touch(&d.join(file), b"onnx");
    touch(&d.join(format!("{file}.json")), json.as_bytes());
    touch(&d.join("tokens.txt"), b"x");
    touch(&d.join("MODEL_CARD"), b"x");
    touch(&d.join("espeak-ng-data/tr_dict"), b"x");
    d
}

const TR_JSON: &str = r#"{"audio":{"sample_rate":22050,"quality":"medium"},"espeak":{"voice":"tr"},"language":{"code":"tr_TR","family":"tr","region":"TR"},"num_speakers":1,"speaker_id_map":{}}"#;

#[test]
fn a_kokoro_folder_is_detected_with_its_54_voices() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = kokoro_dir(tmp.path(), "kokoro-int8-multi-lang-v1_0");
    let m = detect(&dir).unwrap();
    assert_eq!(m.family, TtsFamily::Kokoro);
    assert_eq!(m.name, "kokoro-int8-multi-lang-v1_0");
    assert_eq!(m.quantization, "int8");
    assert_eq!(m.sample_rate, Some(24_000));
    assert_eq!(m.id, dir.join("model.int8.onnx").to_string_lossy());
    assert!(m.voices_file.is_some() && m.data_dir.is_some());
    assert_eq!(m.lexicons.len(), 3);
    assert_eq!(m.rule_fsts.len(), 3);
    assert_eq!(
        m.languages,
        ["en", "es", "fr", "hi", "it", "ja", "pt", "zh"]
    );
    assert_eq!(m.voices.len(), 54);
    assert_eq!(m.voices[3].id, "af_heart");
    assert_eq!(m.voices[3].sid, 3);
    assert_eq!(m.voices[3].language, "en-US");
    assert_eq!(m.voices[3].gender, "female");
    assert_eq!(m.voices[53].id, "em_santa");
    assert_eq!(m.voices[26].language, "en-GB");
    assert_eq!(m.voices[47].language, "zh");
    assert!(m.size_bytes >= 10);
}

#[test]
fn a_piper_folder_is_detected_with_its_language() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = piper_dir(
        tmp.path(),
        "vits-piper-tr_TR-fettah-medium-int8",
        "tr_TR-fettah-medium.onnx",
        TR_JSON,
    );
    let m = detect(&dir).unwrap();
    assert_eq!(m.family, TtsFamily::Vits);
    assert_eq!(m.languages, ["tr"]);
    assert_eq!(m.sample_rate, Some(22_050));
    assert_eq!(m.voices.len(), 1);
    assert_eq!(m.voices[0].id, "fettah");
    assert_eq!(m.voices[0].language, "tr-TR");
    assert_eq!(m.model_file, dir.join("tr_TR-fettah-medium.onnx"));
    // No `.onnx.json`: the folder name carries the language.
    let bare = tmp.path().join("vits-piper-de_DE-thorsten-medium");
    touch(&bare.join("de_DE-thorsten-medium.onnx"), b"x");
    touch(&bare.join("tokens.txt"), b"x");
    touch(&bare.join("espeak-ng-data/de_dict"), b"x");
    let m = detect(&bare).unwrap();
    assert_eq!(m.languages, ["de"]);
    assert_eq!(m.voices[0].id, "thorsten");
}

#[test]
fn multi_speaker_voices_come_from_the_speaker_map() {
    let tmp = tempfile::tempdir().unwrap();
    let json = r#"{"audio":{"sample_rate":22050},"language":{"code":"en_GB"},"num_speakers":3,"speaker_id_map":{"zed":2,"amy":0,"bob":1}}"#;
    let dir = piper_dir(
        tmp.path(),
        "vits-piper-en_GB-vctk-medium",
        "en_GB-vctk-medium.onnx",
        json,
    );
    let m = detect(&dir).unwrap();
    let names: Vec<_> = m.voices.iter().map(|v| (v.id.as_str(), v.sid)).collect();
    assert_eq!(names, [("amy", 0), ("bob", 1), ("zed", 2)]);
    assert_eq!(m.resolve_voice(Some("BOB")).unwrap().sid, 1);
    assert_eq!(m.resolve_voice(Some("2")).unwrap().id, "zed");
    let e = m.resolve_voice(Some("nobody")).unwrap_err();
    assert!(e.starts_with("tts.voice_not_found"), "{e}");
    assert!(e.contains("amy, bob, zed"), "{e}");
}

#[test]
fn folders_that_are_not_runnable_models_are_not_detected() {
    let tmp = tempfile::tempdir().unwrap();
    // Matcha and Kitten have tokens + onnx too but another layout.
    for name in ["matcha-icefall-en_US-ljspeech", "kitten-nano-en-v0_1-fp16"] {
        let d = tmp.path().join(name);
        touch(&d.join("model.onnx"), b"x");
        touch(&d.join("tokens.txt"), b"x");
        touch(&d.join("voices.bin"), b"x");
        touch(&d.join("espeak-ng-data/x"), b"x");
        assert!(detect(&d).is_none(), "{name}");
    }
    // voices.bin without kokoro in the name.
    let d = tmp.path().join("some-voices");
    touch(&d.join("model.onnx"), b"x");
    touch(&d.join("tokens.txt"), b"x");
    touch(&d.join("voices.bin"), b"x");
    assert!(detect(&d).is_none());
    // No tokens, an onnx alone, a whisper folder.
    let d = tmp.path().join("lonely");
    touch(&d.join("model.onnx"), b"x");
    assert!(detect(&d).is_none());
    assert!(detect(&tmp.path().join("missing")).is_none());
}

#[test]
fn scan_finds_downloaded_and_unpacked_models_once() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    kokoro_dir(&root.join("tts"), "kokoro-int8-multi-lang-v1_0");
    piper_dir(
        &root.join("elsewhere/deeper"),
        "vits-piper-tr_TR-fettah-medium-int8",
        "tr_TR-fettah-medium.onnx",
        TR_JSON,
    );
    // Hidden staging folders and the model's own sub-folders are not models.
    kokoro_dir(&root.join("tts/.staging-x"), "kokoro-int8-multi-lang-v1_0");
    let found = scan(&[root.clone(), root.clone()]);
    let names: Vec<_> = found.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "kokoro-int8-multi-lang-v1_0",
            "vits-piper-tr_TR-fettah-medium-int8"
        ]
    );
    let lib = library_models(std::slice::from_ref(&root));
    assert_eq!(lib.len(), 2);
    assert!(lib.iter().all(|m| m.kind == crate::domain::ModelKind::Tts));
    let kokoro = lib.iter().find(|m| m.name.starts_with("kokoro")).unwrap();
    assert_eq!(kokoro.architecture.as_deref(), Some("kokoro"));
    assert_eq!(kokoro.quantization, "int8");
    assert_eq!(kokoro.filename, "model.int8.onnx");
    // Top-level files are listed for the delete preview; the data folder goes with the dir.
    assert!(kokoro.shard_paths.iter().any(|p| p.ends_with("voices.bin")));
    assert!(kokoro.shard_paths.iter().all(|p| p.is_file()));
}

#[test]
fn deleting_a_model_removes_its_folder_inside_a_root_only() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("models");
    let dir = kokoro_dir(&root.join("tts"), "kokoro-int8-multi-lang-v1_0");
    let outside = tempfile::tempdir().unwrap();
    let other = kokoro_dir(outside.path(), "kokoro-int8-multi-lang-v1_0");
    let roots = vec![root.clone()];
    let e = delete_model_dir(&other.join("model.int8.onnx"), &roots).unwrap_err();
    assert!(e.starts_with("library.outside_library"), "{e}");
    assert!(other.exists());
    delete_model_dir(&dir.join("model.int8.onnx"), &roots).unwrap();
    assert!(!dir.exists());
    assert!(root.join("tts").exists());
    // A file that is not in a model folder is refused.
    touch(&root.join("tts/x/readme.txt"), b"x");
    assert!(delete_model_dir(&root.join("tts/x/readme.txt"), &roots).is_err());
}

fn models() -> (tempfile::TempDir, Vec<TtsModel>) {
    let tmp = tempfile::tempdir().unwrap();
    piper_dir(
        tmp.path(),
        "vits-piper-tr_TR-fettah-medium-int8",
        "tr_TR-fettah-medium.onnx",
        TR_JSON,
    );
    piper_dir(
        tmp.path(),
        "vits-piper-tr_TR-fahrettin-medium-int8",
        "tr_TR-fahrettin-medium.onnx",
        TR_JSON,
    );
    kokoro_dir(tmp.path(), "kokoro-int8-multi-lang-v1_0");
    let found = scan(&[tmp.path().to_path_buf()]);
    (tmp, found)
}

#[test]
fn the_model_field_picks_a_model() {
    let (_tmp, found) = models();
    // Nothing or an OpenAI name: the default, Kokoro first.
    for wanted in [None, Some(""), Some("tts-1"), Some("TTS-1-HD")] {
        assert_eq!(
            find_model(&found, wanted).unwrap().family,
            TtsFamily::Kokoro
        );
    }
    assert_eq!(
        find_model(&found, Some("vits-piper-tr_TR-fettah-medium-int8"))
            .unwrap()
            .name,
        "vits-piper-tr_TR-fettah-medium-int8"
    );
    // A part of the name that is unambiguous.
    assert_eq!(
        find_model(&found, Some("Fettah")).unwrap().voices[0].id,
        "fettah"
    );
    assert_eq!(
        find_model(&found, Some("kokoro")).unwrap().family,
        TtsFamily::Kokoro
    );
    let e = find_model(&found, Some("tr_TR")).unwrap_err();
    assert!(e.starts_with("tts.model_ambiguous"), "{e}");
    let e = find_model(&found, Some("whisper-1")).unwrap_err();
    assert!(e.starts_with("tts.model_not_found"), "{e}");
    let e = find_model(&[], None).unwrap_err();
    assert_eq!(e, "tts.no_model");
    // Only a Piper model installed: the default is that one.
    let only: Vec<TtsModel> = found
        .iter()
        .filter(|m| m.family == TtsFamily::Vits)
        .cloned()
        .collect();
    assert_eq!(
        find_model(&only, Some("tts-1")).unwrap().family,
        TtsFamily::Vits
    );
}

#[test]
fn kokoro_voices_resolve_from_names_openai_names_and_numbers() {
    let (_tmp, found) = models();
    let kokoro = found
        .iter()
        .find(|m| m.family == TtsFamily::Kokoro)
        .unwrap();
    assert_eq!(kokoro.resolve_voice(None).unwrap().id, "af_heart");
    assert_eq!(kokoro.resolve_voice(Some("  ")).unwrap().id, "af_heart");
    assert_eq!(kokoro.resolve_voice(Some("AM_ADAM")).unwrap().sid, 11);
    assert_eq!(kokoro.resolve_voice(Some("alloy")).unwrap().id, "af_alloy");
    assert_eq!(kokoro.resolve_voice(Some("echo")).unwrap().id, "am_echo");
    assert_eq!(kokoro.resolve_voice(Some("fable")).unwrap().id, "bm_fable");
    assert_eq!(kokoro.resolve_voice(Some("nova")).unwrap().id, "af_nova");
    assert_eq!(
        kokoro.resolve_voice(Some("shimmer")).unwrap().id,
        "af_bella"
    );
    assert_eq!(kokoro.resolve_voice(Some("30")).unwrap().id, "ff_siwis");
    let e = kokoro.resolve_voice(Some("morgan")).unwrap_err();
    assert!(e.starts_with("tts.voice_not_found"), "{e}");
    assert!(e.contains("af_heart"), "{e}");
    assert!(kokoro.resolve_voice(Some("99")).is_err());
    // Every OpenAI voice name stands for a voice the model has.
    for (openai, _) in OPENAI_VOICES {
        assert!(kokoro.resolve_voice(Some(openai)).is_ok(), "{openai}");
    }
}

#[test]
fn a_single_speaker_model_answers_to_any_voice_name() {
    let (_tmp, found) = models();
    let tr = found.iter().find(|m| m.name.contains("fettah")).unwrap();
    for wanted in [
        None,
        Some("alloy"),
        Some("anything"),
        Some("fettah"),
        Some("0"),
    ] {
        assert_eq!(tr.resolve_voice(wanted).unwrap().sid, 0);
    }
}

#[test]
fn a_kokoro_folder_of_an_unknown_version_takes_speaker_numbers() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = kokoro_dir(tmp.path(), "kokoro-multi-lang-v1_1");
    let m = detect(&dir).unwrap();
    assert!(m.voices.is_empty());
    assert_eq!(m.languages, ["en", "zh"]);
    assert_eq!(m.speaker_id(Some("7")).unwrap(), (7, None));
    assert_eq!(m.speaker_id(None).unwrap(), (0, None));
    assert!(m.speaker_id(Some("alloy")).is_err());
}

fn params(out: &str) -> Params {
    Params {
        sid: 3,
        speed: 1.25,
        threads: 4,
        output: PathBuf::from(out),
    }
}

fn find_arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .find_map(|a| a.strip_prefix(&format!("{flag}=")))
}

#[test]
fn kokoro_arguments() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = kokoro_dir(tmp.path(), "kokoro-int8-multi-lang-v1_0");
    let m = detect(&dir).unwrap();
    let voice = |id: &str| m.voices.iter().find(|v| v.id == id).unwrap();

    let args = build_args(&m, Some(voice("af_heart")), &params("out.wav"));
    assert_eq!(
        find_arg(&args, "--kokoro-model"),
        Some(dir.join("model.int8.onnx").to_string_lossy().as_ref())
    );
    assert!(find_arg(&args, "--kokoro-voices")
        .unwrap()
        .ends_with("voices.bin"));
    assert!(find_arg(&args, "--kokoro-tokens")
        .unwrap()
        .ends_with("tokens.txt"));
    assert!(find_arg(&args, "--kokoro-data-dir")
        .unwrap()
        .ends_with("espeak-ng-data"));
    // American accent: the US English lexicon, then the Chinese one; no language switch.
    let lexicon = find_arg(&args, "--kokoro-lexicon").unwrap();
    let parts: Vec<&str> = lexicon.split(',').collect();
    assert_eq!(parts.len(), 2);
    assert!(parts[0].ends_with("lexicon-us-en.txt") && parts[1].ends_with("lexicon-zh.txt"));
    assert!(find_arg(&args, "--kokoro-lang").is_none());
    assert!(find_arg(&args, "--tts-rule-fsts").is_none());
    assert_eq!(find_arg(&args, "--sid"), Some("3"));
    assert_eq!(find_arg(&args, "--speed"), Some("1.25"));
    assert_eq!(find_arg(&args, "--num-threads"), Some("4"));
    assert_eq!(find_arg(&args, "--output-filename"), Some("out.wav"));
    assert_eq!(args.last().map(String::as_str), Some("--print-args=false"));

    // British voices use the British lexicon.
    let args = build_args(&m, Some(voice("bf_emma")), &params("o.wav"));
    assert!(find_arg(&args, "--kokoro-lexicon")
        .unwrap()
        .contains("lexicon-gb-en.txt"));
    // Spanish and French voices switch the language.
    let args = build_args(&m, Some(voice("ef_dora")), &params("o.wav"));
    assert_eq!(find_arg(&args, "--kokoro-lang"), Some("es"));
    let args = build_args(&m, Some(voice("ff_siwis")), &params("o.wav"));
    assert_eq!(find_arg(&args, "--kokoro-lang"), Some("fr"));
    let args = build_args(&m, Some(voice("pf_dora")), &params("o.wav"));
    assert_eq!(find_arg(&args, "--kokoro-lang"), Some("pt-br"));
    // Chinese voices add the text-normalisation rules.
    let args = build_args(&m, Some(voice("zf_xiaoxiao")), &params("o.wav"));
    let fsts = find_arg(&args, "--tts-rule-fsts").unwrap();
    assert_eq!(fsts.split(',').count(), 3);
    assert!(find_arg(&args, "--kokoro-lang").is_none());
    // The text is not part of the arguments: it goes last, separately.
    assert!(args.iter().all(|a| a.starts_with("--")));
}

#[test]
fn vits_arguments() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = piper_dir(
        tmp.path(),
        "vits-piper-tr_TR-fettah-medium-int8",
        "tr_TR-fettah-medium.onnx",
        TR_JSON,
    );
    let m = detect(&dir).unwrap();
    let args = build_args(
        &m,
        Some(&m.voices[0]),
        &Params {
            sid: 0,
            speed: 1.0,
            threads: 0,
            output: PathBuf::from("o.wav"),
        },
    );
    assert!(find_arg(&args, "--vits-model")
        .unwrap()
        .ends_with("tr_TR-fettah-medium.onnx"));
    assert!(find_arg(&args, "--vits-tokens")
        .unwrap()
        .ends_with("tokens.txt"));
    assert!(find_arg(&args, "--vits-data-dir")
        .unwrap()
        .ends_with("espeak-ng-data"));
    assert!(find_arg(&args, "--vits-lexicon").is_none());
    assert!(args.iter().all(|a| !a.starts_with("--kokoro")));
    // At least one thread, whatever the caller computed.
    assert_eq!(find_arg(&args, "--num-threads"), Some("1"));
    assert_eq!(find_arg(&args, "--speed"), Some("1"));
}

#[test]
fn vits_with_a_lexicon_instead_of_espeak() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("vits-zh-aishell3");
    touch(&dir.join("vits-aishell3.onnx"), b"x");
    touch(&dir.join("tokens.txt"), b"x");
    touch(&dir.join("lexicon.txt"), b"x");
    let m = detect(&dir).unwrap();
    let args = build_args(&m, None, &params("o.wav"));
    assert!(find_arg(&args, "--vits-lexicon")
        .unwrap()
        .ends_with("lexicon.txt"));
    assert!(find_arg(&args, "--vits-data-dir").is_none());
}

#[test]
fn requests_are_checked_like_openais() {
    assert_eq!(
        check_request("  Hello  ", None).unwrap(),
        ("Hello".into(), 1.0)
    );
    assert_eq!(check_request("Hi", Some(0.25)).unwrap().1, 0.25);
    assert_eq!(check_request("Hi", Some(4.0)).unwrap().1, 4.0);
    for bad in [0.24, 4.01, f32::NAN, f32::INFINITY, -1.0] {
        let e = check_request("Hi", Some(bad)).unwrap_err();
        assert!(e.starts_with("tts.speed_out_of_range"), "{bad}: {e}");
    }
    assert!(check_request("  \n ", None)
        .unwrap_err()
        .starts_with("tts.input_empty"));
    assert!(check_request(&"a".repeat(MAX_INPUT_CHARS), None).is_ok());
    let e = check_request(&"a".repeat(MAX_INPUT_CHARS + 1), None).unwrap_err();
    assert!(e.starts_with("tts.input_too_long"), "{e}");
    // The limit counts characters, not bytes.
    assert!(check_request(&"ş".repeat(MAX_INPUT_CHARS), None).is_ok());
}

#[test]
fn response_formats() {
    for f in AudioFormat::ALL {
        assert_eq!(AudioFormat::parse(f.as_str()), Some(f));
        assert_eq!(AudioFormat::parse(&f.as_str().to_uppercase()), Some(f));
    }
    assert_eq!(AudioFormat::parse("ogg"), None);
    assert_eq!(AudioFormat::parse(""), None);
    assert!(!AudioFormat::Wav.needs_ffmpeg() && !AudioFormat::Pcm.needs_ffmpeg());
    assert!(AudioFormat::Mp3.needs_ffmpeg() && AudioFormat::Opus.needs_ffmpeg());
    assert_eq!(AudioFormat::Mp3.content_type(), "audio/mpeg");
    assert_eq!(AudioFormat::Opus.extension(), "opus");
    assert_eq!(AudioFormat::Wav.extension(), "wav");
}
