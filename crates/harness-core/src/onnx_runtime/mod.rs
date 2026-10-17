//! The managed ONNX Runtime: the shared library the `harness-onnx` sidecar loads.
//!
//! **Why a download and not a bundle.** `harness-onnx` links no ONNX Runtime (`ort` with
//! `load-dynamic`); it loads `onnxruntime.dll` / `libonnxruntime.so` from a folder Harness
//! manages, like the llama.cpp builds. The library is 15-25 MB, the DirectML build with its
//! `DirectML.dll` 36 MB, a CUDA build several hundred MB: bundling them would make every
//! installer pay for a feature most users never open, and a CUDA build cannot be bundled at
//! all (NVIDIA libraries are not redistributable here). So the installer carries only the
//! small sidecar, and the first ONNX model that is loaded (or the Install button in Settings > Runtime > Registry) fetches the
//! library into `<data>/bin/onnxruntime/<variant>-<version>/`.
//!
//! **Integrity.** Every file Harness puts there has a SHA-256 pinned in this source (computed
//! from the official Microsoft release / NuGet packages when the version was chosen), and an
//! install fails when a downloaded byte differs. Archives that cannot be read in parts
//! (`.tgz`) are pinned as a whole (the digest GitHub publishes for the release asset).
//! `manifest.json` records what was installed; [`installed`] re-checks the file sizes on every
//! use.
//!
//! **Variants.** Windows x64: `directml` (ONNX Runtime 1.24.4, the last release Microsoft
//! publishes with the DirectML execution provider, plus DirectML 1.15.4: AMD, NVIDIA and Intel
//! GPUs through DirectX 12, and the CPU as fallback), `cpu` and `cuda` (1.28.3, CUDA 12; opt-in: with
//! it come the CUDA 12.9 runtime, cuBLAS, cuFFT and cuDNN 9.27 DLLs, read out of NVIDIA's
//! `nvidia-*-cu12` PyPI wheels and pinned like the rest, about 2.1 GB on disk; their NVIDIA
//! licence texts are installed beside them. Harness downloads them from NVIDIA's packages on
//! the user's request and does not ship them). Linux x64 and macOS arm64: `cpu`.
//! The sidecar asks ONNX Runtime for API level 24, which every version above serves.
//!
//! **GenAI.** The Windows builds also carry `onnxruntime-genai.dll` (ONNX Runtime GenAI 0.17.0,
//! MIT; the `winml` release asset, which is the build with the DirectML code path and runs on
//! the CPU as well; the CUDA build adds `onnxruntime-genai-cuda.dll`). It is what runs the chat
//! models of Hugging Face's ONNX repos (`genai_config.json` next to the graph): the sidecar loads
//! it from the folder of the ONNX Runtime it is started with. Linux and macOS have no GenAI
//! library yet (chat models need Windows for now).
//!
//! Windows archives are read with HTTP range requests ([`remote_zip`]): only the wanted DLLs
//! come over the wire (7 MB instead of 78 MB for the CPU release, 7 MB instead of 202 MB for
//! DirectML.dll).

pub mod remote_zip;

use crate::error_key::err;
use crate::paths;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Runtime family id of the ONNX engine (`Compatibility::Runnable { runtime }`).
pub const ONNX_FAMILY: &str = "onnxruntime";

/// Which ONNX Runtime build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OrtVariant {
    /// DirectX 12 GPUs (Windows), CPU fallback.
    Directml,
    /// NVIDIA CUDA 12.
    Cuda,
    Cpu,
}

impl OrtVariant {
    pub fn as_str(self) -> &'static str {
        match self {
            OrtVariant::Directml => "directml",
            OrtVariant::Cuda => "cuda",
            OrtVariant::Cpu => "cpu",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "directml" | "dml" => Some(Self::Directml),
            "cuda" => Some(Self::Cuda),
            "cpu" => Some(Self::Cpu),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    /// Read with range requests, files checked one by one.
    Zip,
    /// Downloaded whole, checked as a whole.
    TarGz,
}

/// A file taken out of an archive.
#[derive(Debug, Clone, Copy)]
pub struct FileSpec {
    /// Path inside the archive.
    pub inner: &'static str,
    /// Name in the install folder.
    pub out: &'static str,
    /// SHA-256 of the extracted file (`""` for tarballs, which are pinned whole).
    pub sha256: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ArchiveSpec {
    pub url: &'static str,
    /// Size of the whole archive (progress only).
    pub size: u64,
    /// SHA-256 of the whole archive.
    pub sha256: &'static str,
    pub format: ArchiveFormat,
    pub files: &'static [FileSpec],
}

/// One installable build.
#[derive(Debug, Clone, Copy)]
pub struct VariantSpec {
    pub variant: OrtVariant,
    /// ONNX Runtime version.
    pub version: &'static str,
    pub archives: &'static [ArchiveSpec],
    /// File name of the library the sidecar loads (`--ort-lib`).
    pub lib: &'static str,
    /// Size of the installed files (the extracted files of every archive), measured from the
    /// pinned archives: what the build takes on disk, unlike the download size of the archives.
    pub disk_bytes: u64,
}

/// File name of the GenAI library in an install folder.
#[cfg(windows)]
pub const GENAI_LIB: &str = "onnxruntime-genai.dll";
#[cfg(target_os = "macos")]
pub const GENAI_LIB: &str = "libonnxruntime-genai.dylib";
#[cfg(not(any(windows, target_os = "macos")))]
pub const GENAI_LIB: &str = "libonnxruntime-genai.so";

/// ONNX Runtime GenAI 0.17.0 for Windows x64: DirectML-capable `onnxruntime-genai.dll` (it
/// loads `onnxruntime.dll` and `DirectML.dll` of the same folder on demand). Pinned with the
/// digest GitHub shows for the release asset and the digest of the extracted DLL.
#[cfg(all(windows, target_arch = "x86_64"))]
const GENAI_WIN: ArchiveSpec = ArchiveSpec {
    url: concat!(
        "https://github.com/microsoft/onnxruntime-genai/releases/download/v0.17.0/",
        "onnxruntime-genai-0.17.0-win-x64-winml.zip"
    ),
    size: 8_112_532,
    sha256: "c03cf8bbec9c521e9494df26c14443b74b13d7f83f94a91f9430d551e95f18c1",
    format: ArchiveFormat::Zip,
    files: &[FileSpec {
        inner: "onnxruntime-genai-0.17.0-win-x64-winml/lib/onnxruntime-genai.dll",
        out: "onnxruntime-genai.dll",
        sha256: "f650d6f4f1224e0d1c108f1610a0d8ed732114a859297d50ab2eea8f0538e3ae",
    }],
};

/// The same release asset for the CUDA build, which also needs the CUDA execution provider
/// library of GenAI.
#[cfg(all(windows, target_arch = "x86_64"))]
const GENAI_WIN_CUDA: ArchiveSpec = ArchiveSpec {
    files: &[
        FileSpec {
            inner: "onnxruntime-genai-0.17.0-win-x64-winml/lib/onnxruntime-genai.dll",
            out: "onnxruntime-genai.dll",
            sha256: "f650d6f4f1224e0d1c108f1610a0d8ed732114a859297d50ab2eea8f0538e3ae",
        },
        FileSpec {
            inner: "onnxruntime-genai-0.17.0-win-x64-winml/lib/onnxruntime-genai-cuda.dll",
            out: "onnxruntime-genai-cuda.dll",
            sha256: "788aff404b557f8bc409dfb0e056e2dbe0e5788f1a69ffb2b861954f5b8dda9d",
        },
    ],
    ..GENAI_WIN
};

#[cfg(all(windows, target_arch = "x86_64"))]
const SPECS: &[VariantSpec] = &[
    VariantSpec {
        variant: OrtVariant::Directml,
        version: "1.24.4",
        lib: "onnxruntime.dll",
        disk_bytes: 44_572_240,
        archives: &[
            ArchiveSpec {
                url: "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.24.4/microsoft.ml.onnxruntime.directml.1.24.4.nupkg",
                size: 12_458_649,
                sha256: "57e9f11b73437bef7a309496135d4c1f96b1a8e9ddba60013fa27bfc1d788681",
                format: ArchiveFormat::Zip,
                files: &[
                    FileSpec {
                        inner: "runtimes/win-x64/native/onnxruntime.dll",
                        out: "onnxruntime.dll",
                        sha256: "e7eedec6a6f26dc39dc948276a75ef6d2bee3fff944d874ceed0bbd3b97bff40",
                    },
                    FileSpec {
                        inner: "runtimes/win-x64/native/onnxruntime_providers_shared.dll",
                        out: "onnxruntime_providers_shared.dll",
                        sha256: "265c8daf29637cb259cac8be9f08f2cd45f3883f0f0e4949cbfddd5b4cbec3b6",
                    },
                ],
            },
            ArchiveSpec {
                url: "https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/1.15.4/microsoft.ai.directml.1.15.4.nupkg",
                size: 202_292_617,
                sha256: "4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9",
                format: ArchiveFormat::Zip,
                files: &[FileSpec {
                    inner: "bin/x64-win/DirectML.dll",
                    out: "DirectML.dll",
                    sha256: "9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1",
                }],
            },
            GENAI_WIN,
        ],
    },
    VariantSpec {
        variant: OrtVariant::Cpu,
        version: "1.28.3",
        lib: "onnxruntime.dll",
        disk_bytes: 24_544_960,
        archives: &[ArchiveSpec {
            url: concat!(
                "https://github.com/microsoft/onnxruntime/releases/download/v1.28.3/",
                "onnxruntime-win-x64-1.28.3.zip"
            ),
            size: 78_606_946,
            sha256: "1d6fab48e85f948436af7c8c971d2c145cf224e2c444755dec894f8b0de11a83",
            format: ArchiveFormat::Zip,
            files: &[
                FileSpec {
                    inner: "onnxruntime-win-x64-1.28.3/lib/onnxruntime.dll",
                    out: "onnxruntime.dll",
                    sha256: "4d2774a5f64e4a230b16b74b167171e92f65b0683c19680c1d04fb284b92a6e2",
                },
                FileSpec {
                    inner: "onnxruntime-win-x64-1.28.3/lib/onnxruntime_providers_shared.dll",
                    out: "onnxruntime_providers_shared.dll",
                    sha256: "6582418469facb25b33bc212d8efdc8f7ee57d0c8161e57a59434377780a38b4",
                },
            ],
        },
        GENAI_WIN],
    },
    VariantSpec {
        variant: OrtVariant::Cuda,
        version: "1.28.3",
        lib: "onnxruntime.dll",
        disk_bytes: 2_734_212_780,
        archives: &[ArchiveSpec {
            url: concat!(
                "https://github.com/microsoft/onnxruntime/releases/download/v1.28.3/",
                "onnxruntime-win-x64-gpu_cuda12-1.28.3.zip"
            ),
            size: 454_803_675,
            sha256: "65d7f956649df80a8ccdcafa7843e4b02f38bfc9e6de28711ed8384508040330",
            format: ArchiveFormat::Zip,
            files: &[
                FileSpec {
                    inner: "onnxruntime-win-x64-gpu_cuda12-1.28.3/lib/onnxruntime.dll",
                    out: "onnxruntime.dll",
                    sha256: "40f8e2ef3bb42d9aa33041de3c83e040450e17dcef534b07aa771adb7518c060",
                },
                FileSpec {
                    inner: "onnxruntime-win-x64-gpu_cuda12-1.28.3/lib/onnxruntime_providers_shared.dll",
                    out: "onnxruntime_providers_shared.dll",
                    sha256: "acb3d562836ba5d139c4b323724cbe08ac0b9e1352089eecf82a525aa5a7f40f",
                },
                FileSpec {
                    inner: "onnxruntime-win-x64-gpu_cuda12-1.28.3/lib/onnxruntime_providers_cuda.dll",
                    out: "onnxruntime_providers_cuda.dll",
                    sha256: "d1354ce9f082972dcd957a7472d95e37e20625a3f1c2a24db56b9db611dbdd9b",
                },
            ],
        },
        GENAI_WIN_CUDA,
        // The CUDA 12 and cuDNN 9 libraries the CUDA provider loads, from NVIDIA's PyPI wheels
        // (only these DLLs and the licence texts are read out of them). The sidecar puts the
        // install folder on its own DLL search path, so nothing system-wide changes.
        ArchiveSpec {
            url: "https://files.pythonhosted.org/packages/59/df/e7c3a360be4f7b93cee39271b792669baeb3846c58a4df6dfcf187a7ffab/nvidia_cuda_runtime_cu12-12.9.79-py3-none-win_amd64.whl",
            size: 3_591_604,
            sha256: "8e018af8fa02363876860388bd10ccb89eb9ab8fb0aa749aaf58430a9f7c4891",
            format: ArchiveFormat::Zip,
            files: &[
                FileSpec {
                    inner: "nvidia/cuda_runtime/bin/cudart64_12.dll",
                    out: "cudart64_12.dll",
                    sha256: "760c38928bbe5759f7b31ed6692599eb7ec83cedd5702e84c2b72028a89837e1",
                },
                FileSpec {
                    inner: "nvidia_cuda_runtime_cu12-12.9.79.dist-info/licenses/License.txt",
                    out: "NVIDIA-CUDA-LICENSE.txt",
                    sha256: "ad6f5853fba0ca0d159d0f58d49ae49830c2f8c93f7a92648b9ce90adb4c6ccd",
                },
            ],
        },
        ArchiveSpec {
            url: "https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl",
            size: 553_162_896,
            sha256: "623f43027d40d44ceadf0043f002bd25cf353e8f13ce90b9a87057019f560661",
            format: ArchiveFormat::Zip,
            files: &[
                FileSpec {
                    inner: "nvidia/cublas/bin/cublas64_12.dll",
                    out: "cublas64_12.dll",
                    sha256: "52ce5ba0ec5327d39be6021f0f9362d2ed4d64d7206f9b5afc06398d827b8e82",
                },
                FileSpec {
                    inner: "nvidia/cublas/bin/cublasLt64_12.dll",
                    out: "cublasLt64_12.dll",
                    sha256: "71705a7cf0923f4ab034d0d2620bbfc989ff669ac3d79f868b7a7c597ca559c5",
                },
            ],
        },
        ArchiveSpec {
            url: "https://files.pythonhosted.org/packages/20/ee/29955203338515b940bd4f60ffdbc073428f25ef9bfbce44c9a066aedc5c/nvidia_cufft_cu12-11.4.1.4-py3-none-win_amd64.whl",
            size: 200_067_309,
            sha256: "8e5bfaac795e93f80611f807d42844e8e27e340e0cde270dcb6c65386d795b80",
            format: ArchiveFormat::Zip,
            files: &[
                FileSpec {
                    inner: "nvidia/cufft/bin/cufft64_11.dll",
                    out: "cufft64_11.dll",
                    sha256: "5b90655ca9cdf5d91cbdf2c72b995ec937eed0aef2d6954370c711b45c929b35",
                },
            ],
        },
        ArchiveSpec {
            url: "https://files.pythonhosted.org/packages/aa/38/f856579877f7c1c5066e61182e7de7bc27bf35a78c8d1b0fa592e6985bc4/nvidia_cudnn_cu12-9.27.0.42-py3-none-win_amd64.whl",
            size: 743_068_852,
            sha256: "06e9b0026f3bad97d2b58666330fabec04fe1672f776661ecb0ce0029c27f142",
            format: ArchiveFormat::Zip,
            files: &[
                FileSpec {
                    inner: "nvidia/cudnn/bin/cudnn64_9.dll",
                    out: "cudnn64_9.dll",
                    sha256: "7268ef53e32c2c574372326dfc9d94a26095f4ed9c89dba87cfb3ab1e40a6470",
                },
                FileSpec {
                    inner: "nvidia/cudnn/bin/cudnn_adv64_9.dll",
                    out: "cudnn_adv64_9.dll",
                    sha256: "712ace96559e60b5be81a1097acb7609f601ff4cd0cdef848b013d671753b4b4",
                },
                FileSpec {
                    inner: "nvidia/cudnn/bin/cudnn_cnn64_9.dll",
                    out: "cudnn_cnn64_9.dll",
                    sha256: "dff4c446c855b3ab2d486aebf4c11388cf74da14d7bd0bfafb61c3a75481313d",
                },
                FileSpec {
                    inner: "nvidia/cudnn/bin/cudnn_engines_precompiled64_9.dll",
                    out: "cudnn_engines_precompiled64_9.dll",
                    sha256: "6b603ac45fc018a96f014d0c6cdbc8f480dce2dbe33b8ca38228a4e4c531f3fe",
                },
                FileSpec {
                    inner: "nvidia/cudnn/bin/cudnn_engines_runtime_compiled64_9.dll",
                    out: "cudnn_engines_runtime_compiled64_9.dll",
                    sha256: "f92f4776261e4409aa9f84357c40bc3626700292a1d22025746388bea43ae11b",
                },
                FileSpec {
                    inner: "nvidia/cudnn/bin/cudnn_engines_tensor_ir64_9.dll",
                    out: "cudnn_engines_tensor_ir64_9.dll",
                    sha256: "d7380a7c4ad90590968e97b4d8b43a616e7e0070bfdeeee04c4d0be85ad52394",
                },
                FileSpec {
                    inner: "nvidia/cudnn/bin/cudnn_ext64_9.dll",
                    out: "cudnn_ext64_9.dll",
                    sha256: "f20888488b701e429830e19cf59022e90a0c5dd398723162ea9f320b3ca06636",
                },
                FileSpec {
                    inner: "nvidia/cudnn/bin/cudnn_graph64_9.dll",
                    out: "cudnn_graph64_9.dll",
                    sha256: "445e8c3e44a6a6035749380009ae35ae8c2bc246428229b79545a25238bb56e2",
                },
                FileSpec {
                    inner: "nvidia/cudnn/bin/cudnn_heuristic64_9.dll",
                    out: "cudnn_heuristic64_9.dll",
                    sha256: "0634af738266cd36c912e99cd3807ee452450e963bf885be204fdcfb322cfc70",
                },
                FileSpec {
                    inner: "nvidia/cudnn/bin/cudnn_ops64_9.dll",
                    out: "cudnn_ops64_9.dll",
                    sha256: "390bfe103a244c714fca5a1320950ba51a3173a7fbd6664ed535ce7367891ccd",
                },
                FileSpec {
                    inner: "nvidia_cudnn_cu12-9.27.0.42.dist-info/licenses/License.txt",
                    out: "NVIDIA-CUDNN-LICENSE.txt",
                    sha256: "49cf79bdb35734b52fe6203013b3bd759f81e998cd32aa2c65c51db9a88c61d2",
                },
            ],
        },
        ],
    },
];

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const SPECS: &[VariantSpec] = &[VariantSpec {
    variant: OrtVariant::Cpu,
    version: "1.28.3",
    lib: "libonnxruntime.so",
    disk_bytes: 24_316_248,
    archives: &[ArchiveSpec {
        url: concat!(
            "https://github.com/microsoft/onnxruntime/releases/download/v1.28.3/",
            "onnxruntime-linux-x64-1.28.3.tgz"
        ),
        size: 9_130_098,
        sha256: "db14e4863bd37893fc59729d986ab2a0d043d10b7d44da1913c4982b7e3d009c",
        format: ArchiveFormat::TarGz,
        files: &[
            FileSpec {
                inner: "onnxruntime-linux-x64-1.28.3/lib/libonnxruntime.so.1.28.3",
                out: "libonnxruntime.so",
                sha256: "",
            },
            FileSpec {
                inner: "onnxruntime-linux-x64-1.28.3/lib/libonnxruntime_providers_shared.so",
                out: "libonnxruntime_providers_shared.so",
                sha256: "",
            },
        ],
    }],
}];

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const SPECS: &[VariantSpec] = &[VariantSpec {
    variant: OrtVariant::Cpu,
    version: "1.28.3",
    lib: "libonnxruntime.dylib",
    disk_bytes: 39_350_024,
    archives: &[ArchiveSpec {
        url: concat!(
            "https://github.com/microsoft/onnxruntime/releases/download/v1.28.3/",
            "onnxruntime-osx-arm64-1.28.3.tgz"
        ),
        size: 33_057_743,
        sha256: "c436bd9f47dbce6f6311ccc21de97f829b3c862c09d24aa321958cb72f0a1d32",
        format: ArchiveFormat::TarGz,
        files: &[FileSpec {
            inner: "onnxruntime-osx-arm64-1.28.3/lib/libonnxruntime.1.28.3.dylib",
            out: "libonnxruntime.dylib",
            sha256: "",
        }],
    }],
}];

#[cfg(not(any(
    all(windows, target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64")
)))]
const SPECS: &[VariantSpec] = &[];

/// Builds available on this OS/architecture.
pub fn specs() -> &'static [VariantSpec] {
    SPECS
}

/// Whether ONNX Runtime runs here at all: this OS/architecture has at least one pinned build.
/// Intel Macs have none (Microsoft's last x86_64 / universal2 macOS release is 1.23.2, below
/// the API level 24 the sidecar asks for), so ONNX models cannot run there.
pub fn supported() -> bool {
    !specs().is_empty()
}

/// [`supported`] with the OS and architecture it was decided for (the UI names them).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PlatformSupport {
    pub supported: bool,
    /// `std::env::consts::OS` (`macos`, `windows`, `linux`).
    pub os: String,
    /// `std::env::consts::ARCH` (`x86_64`, `aarch64`).
    pub arch: String,
}

pub fn platform_support() -> PlatformSupport {
    PlatformSupport {
        supported: supported(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

/// The error for a build this platform does not have: `onnx.platform_unsupported` when it has
/// none at all, else `onnx.unsupported_platform` for that one variant.
fn no_spec(variant: OrtVariant) -> String {
    if supported() {
        err("onnx.unsupported_platform")
            .with("variant", variant.as_str())
            .into()
    } else {
        err("onnx.platform_unsupported")
            .with("os", std::env::consts::OS)
            .with("arch", std::env::consts::ARCH)
            .into()
    }
}

pub fn spec_for(variant: OrtVariant) -> Option<&'static VariantSpec> {
    specs().iter().find(|s| s.variant == variant)
}

/// The build used when nothing asks for a particular one: DirectML on Windows (it covers AMD,
/// NVIDIA and Intel GPUs), else the CPU build.
pub fn default_variant() -> OrtVariant {
    specs()
        .iter()
        .map(|s| s.variant)
        .find(|v| *v == OrtVariant::Directml)
        .unwrap_or(OrtVariant::Cpu)
}

/// The build the sidecar needs for `--provider <choice>` (`auto`, `cpu`, `directml`, `cuda`).
/// A CPU choice reuses any build that is already installed (every build runs on the CPU).
/// `auto` takes the CUDA build once the user installed it (an opt-in download, so the NVIDIA
/// GPU is the one they want), else the platform default.
pub fn variant_for_provider(choice: &str) -> OrtVariant {
    variant_for_provider_in(&root(), choice)
}

fn variant_for_provider_in(root: &Path, choice: &str) -> OrtVariant {
    match choice.trim().to_ascii_lowercase().as_str() {
        "directml" | "dml" => OrtVariant::Directml,
        "cuda" => OrtVariant::Cuda,
        "cpu" => specs()
            .iter()
            .map(|s| s.variant)
            .find(|v| installed_in(root, *v).is_some())
            .unwrap_or_else(default_variant),
        _ if installed_in(root, OrtVariant::Cuda).is_some() => OrtVariant::Cuda,
        _ => default_variant(),
    }
}

/// The build a launch with `--provider <choice>` fetches when it is missing. The Windows CUDA
/// build (about 2.1 GB with its CUDA and cuDNN libraries) is installed only from Settings, so
/// a CUDA launch without it takes the DirectML build instead (the engine says so in the log).
pub fn launch_variant(choice: &str) -> OrtVariant {
    launch_variant_in(&root(), choice)
}

fn launch_variant_in(root: &Path, choice: &str) -> OrtVariant {
    match variant_for_provider_in(root, choice) {
        OrtVariant::Cuda if cfg!(windows) && installed_in(root, OrtVariant::Cuda).is_none() => {
            default_variant()
        }
        v => v,
    }
}

/// `<data>/bin/onnxruntime`.
pub fn root() -> PathBuf {
    paths::bin_dir().join("onnxruntime")
}

fn dir_of(root: &Path, spec: &VariantSpec) -> PathBuf {
    root.join(format!("{}-{}", spec.variant.as_str(), spec.version))
}

/// One file of an installed build.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InstalledFile {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// An archive a build came from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InstalledSource {
    pub url: String,
    pub sha256: String,
}

/// `manifest.json` of an installed build.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Manifest {
    pub variant: OrtVariant,
    pub version: String,
    pub files: Vec<InstalledFile>,
    pub sources: Vec<InstalledSource>,
    pub installed_at: u64,
}

/// An installed build.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Installed {
    pub variant: OrtVariant,
    pub version: String,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub dir: PathBuf,
    /// The library the sidecar loads.
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub lib: PathBuf,
    pub files: Vec<InstalledFile>,
    pub size_bytes: u64,
    pub installed_at: u64,
}

fn read_manifest(dir: &Path) -> Option<Manifest> {
    serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).ok()?).ok()
}

/// [`installed`] under `root`: the manifest exists and every file is there with its size.
pub fn installed_in(root: &Path, variant: OrtVariant) -> Option<Installed> {
    let spec = spec_for(variant)?;
    let dir = dir_of(root, spec);
    let m = read_manifest(&dir)?;
    if m.variant != variant || m.version != spec.version {
        return None;
    }
    // Every file the pinned spec names is in the manifest (an install from before GenAI was
    // added lacks the GenAI library and is installed again), and on disk with its size.
    let all_named = spec
        .archives
        .iter()
        .flat_map(|a| a.files.iter())
        .all(|f| m.files.iter().any(|i| i.name == f.out));
    let complete = all_named
        && m.files
            .iter()
            .all(|f| std::fs::metadata(dir.join(&f.name)).is_ok_and(|md| md.len() == f.size));
    let lib = dir.join(spec.lib);
    if !complete || !lib.is_file() {
        return None;
    }
    Some(Installed {
        variant,
        version: m.version,
        size_bytes: m.files.iter().map(|f| f.size).sum(),
        installed_at: m.installed_at,
        files: m.files,
        lib,
        dir,
    })
}

/// Files of an install of `variant` under `root` that are missing or no longer have the size the
/// install recorded: a build that was installed and then changed on disk (an antivirus
/// quarantine, a partial copy). Empty when the build is complete or was never installed (no
/// manifest of this version). A changed file of the same size is caught by [`verify_in`] (and
/// by the CUDA launch, which falls back to DirectML when the provider cannot load).
pub fn damaged_in(root: &Path, variant: OrtVariant) -> Vec<String> {
    let Some(spec) = spec_for(variant) else {
        return Vec::new();
    };
    let dir = dir_of(root, spec);
    let Some(m) = read_manifest(&dir) else {
        return Vec::new();
    };
    if m.variant != variant || m.version != spec.version {
        return Vec::new();
    }
    m.files
        .iter()
        .filter(|f| !std::fs::metadata(dir.join(&f.name)).is_ok_and(|md| md.len() == f.size))
        .map(|f| f.name.clone())
        .collect()
}

/// [`damaged_in`] in the managed folder.
pub fn damaged(variant: OrtVariant) -> Vec<String> {
    damaged_in(&root(), variant)
}

/// The installed build of `variant`, if complete.
pub fn installed(variant: OrtVariant) -> Option<Installed> {
    installed_in(&root(), variant)
}

/// Whether any build of this platform is installed in `root`.
pub fn any_installed_in(root: &Path) -> bool {
    specs()
        .iter()
        .any(|s| installed_in(root, s.variant).is_some())
}

/// Where a build lives, whether or not it is installed.
pub fn target_dir(variant: OrtVariant) -> Option<PathBuf> {
    spec_for(variant).map(|s| dir_of(&root(), s))
}

/// Re-hashes every file of an installed build against its manifest (Settings > Runtime >
/// Verify). Blocking. Returns the names of the files that differ.
pub fn verify_in(root: &Path, variant: OrtVariant) -> Result<Vec<String>, String> {
    let installed = installed_in(root, variant).ok_or_else(|| {
        String::from(err("onnx.runtime_missing").with("variant", variant.as_str()))
    })?;
    let mut changed = Vec::new();
    for f in &installed.files {
        let actual = crate::downloader::sha256_file(&installed.dir.join(&f.name))?;
        if actual != f.sha256 {
            changed.push(f.name.clone());
        }
    }
    Ok(changed)
}

/// Removes an installed build and tells the views (`engines_changed`).
pub fn remove(variant: OrtVariant) -> Result<(), String> {
    remove_in(&root(), variant)?;
    crate::events::publish(crate::events::CoreEvent::EnginesChanged);
    Ok(())
}

/// Removes an installed build.
pub fn remove_in(root: &Path, variant: OrtVariant) -> Result<(), String> {
    let spec = spec_for(variant).ok_or_else(|| no_spec(variant))?;
    let dir = dir_of(root, spec);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| {
            String::from(
                err("fs.remove_failed")
                    .with("path", dir.display())
                    .with("error", e),
            )
        })?;
    }
    Ok(())
}

/// Download progress of the running install (`None` when idle).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InstallProgress {
    pub variant: Option<OrtVariant>,
    pub current: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

static PROGRESS: Mutex<Option<InstallProgress>> = Mutex::new(None);
/// One install at a time.
static INSTALLING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn progress() -> Option<InstallProgress> {
    PROGRESS.lock().unwrap().clone()
}

fn set_progress(f: impl FnOnce(&mut InstallProgress)) {
    if let Some(p) = PROGRESS.lock().unwrap().as_mut() {
        f(p);
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("HarnessStudio/0.1.1")
        .build()
        .map_err(|e| crate::http_error::transport(&e, "HTTP client"))
}

fn hex(bytes: &[u8]) -> String {
    crate::hex::lower(&Sha256::digest(bytes))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| {
        String::from(
            err("fs.write_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })
}

fn digest_mismatch(name: &str, expected: &str, actual: &str) -> String {
    err("runtime.digest_mismatch")
        .with("file", name)
        .with("expected", expected)
        .with("actual", actual)
        .into()
}

/// Downloads `url` into `dest`, returning its SHA-256. Progress counts into the running
/// install.
async fn download_whole(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, url))?;
    if !res.status().is_success() {
        return Err(err("http.status")
            .with("status", res.status())
            .with("url", url)
            .into());
    }
    let mut file = tokio::fs::File::create(dest).await.map_err(|e| {
        String::from(
            err("fs.write_failed")
                .with("path", dest.display())
                .with("error", e),
        )
    })?;
    let mut hasher = Sha256::new();
    let mut stream = res.bytes_stream();
    let mut got = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| crate::http_error::transport(&e, url))?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", dest.display())
                    .with("error", e),
            )
        })?;
        got += chunk.len() as u64;
        set_progress(|p| p.downloaded_bytes = got);
    }
    file.flush().await.ok();
    Ok(crate::hex::lower(&hasher.finalize()))
}

/// Takes the wanted files out of a `.tgz`.
fn extract_tar_gz(archive: &Path, files: &[FileSpec]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use std::io::Read;
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut found: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(spec) = files.iter().find(|f| f.inner == path) {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            found.push((spec.out.to_string(), bytes));
        }
    }
    for f in files {
        if !found.iter().any(|(n, _)| n == f.out) {
            return Err(format!("{} is not in the archive", f.inner));
        }
    }
    Ok(found)
}

/// Files of one ZIP archive: range requests first, the whole archive when the server does not
/// do ranges. Every file is checked against its pinned SHA-256.
async fn fetch_zip_files(
    client: &reqwest::Client,
    archive: &ArchiveSpec,
    staging: &Path,
    base: u64,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let wanted: Vec<&str> = archive.files.iter().map(|f| f.inner).collect();
    let on_bytes = move |n: u64| set_progress(|p| p.downloaded_bytes = base + n);
    let ranged = remote_zip::fetch_entries(client, archive.url, &wanted, &on_bytes).await;
    let contents = match ranged {
        Ok(c) => c,
        Err(partial_error) => {
            // No ranges (or an unreadable table of contents): the archive as a whole, checked
            // against its own digest.
            let part = staging.join(".archive.part");
            let digest = download_whole(client, archive.url, &part)
                .await
                .map_err(|e| format!("{} (range requests failed first: {})", e, partial_error))?;
            if digest != archive.sha256 {
                let _ = std::fs::remove_file(&part);
                return Err(digest_mismatch(archive.url, archive.sha256, &digest));
            }
            let part2 = part.clone();
            let names: Vec<String> = wanted.iter().map(|s| s.to_string()).collect();
            let read =
                tokio::task::spawn_blocking(move || -> Result<Vec<(String, Vec<u8>)>, String> {
                    let mut zip = zip::ZipArchive::new(
                        std::fs::File::open(&part2).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    crate::zip_safety::validate_archive(&mut zip)?;
                    let mut out = Vec::new();
                    for name in names {
                        let mut f = zip.by_name(&name).map_err(|e| format!("{}: {}", name, e))?;
                        let mut bytes = Vec::new();
                        std::io::Read::read_to_end(&mut f, &mut bytes)
                            .map_err(|e| e.to_string())?;
                        out.push((name, bytes));
                    }
                    Ok(out)
                })
                .await
                .map_err(|e| e.to_string())??;
            let _ = std::fs::remove_file(&part);
            read
        }
    };
    let mut out = Vec::new();
    for spec in archive.files {
        let bytes = contents
            .iter()
            .find(|(n, _)| n == spec.inner)
            .map(|(_, b)| b.clone())
            .ok_or_else(|| format!("{} was not read", spec.inner))?;
        let actual = hex(&bytes);
        if actual != spec.sha256 {
            return Err(digest_mismatch(spec.out, spec.sha256, &actual));
        }
        out.push((spec.out.to_string(), bytes));
    }
    Ok(out)
}

/// Installs `variant` under `root` (a no-op when it is already complete).
pub async fn install_in(root: &Path, variant: OrtVariant) -> Result<Installed, String> {
    let spec = spec_for(variant).ok_or_else(|| no_spec(variant))?;
    let _one_at_a_time = INSTALLING.lock().await;
    if let Some(done) = installed_in(root, variant) {
        return Ok(done);
    }
    let total: u64 = spec.archives.iter().map(|a| a.size).sum();
    *PROGRESS.lock().unwrap() = Some(InstallProgress {
        variant: Some(variant),
        current: spec.version.to_string(),
        downloaded_bytes: 0,
        total_bytes: total,
    });
    let result = install_inner(root, spec).await;
    *PROGRESS.lock().unwrap() = None;
    result
}

async fn install_inner(root: &Path, spec: &'static VariantSpec) -> Result<Installed, String> {
    let dir = dir_of(root, spec);
    let staging = root.join(format!(".staging-{}", spec.variant.as_str()));
    let _ = tokio::fs::remove_dir_all(&staging).await;
    tokio::fs::create_dir_all(&staging).await.map_err(|e| {
        String::from(
            err("fs.create_dir_failed")
                .with("path", staging.display())
                .with("error", e),
        )
    })?;
    let client = client()?;
    let outcome = async {
        let mut files: Vec<InstalledFile> = Vec::new();
        let mut sources = Vec::new();
        let mut base = 0u64;
        for archive in spec.archives {
            set_progress(|p| {
                p.current = archive.url.rsplit('/').next().unwrap_or("").to_string();
            });
            let contents = match archive.format {
                ArchiveFormat::Zip => fetch_zip_files(&client, archive, &staging, base).await?,
                ArchiveFormat::TarGz => {
                    let part = staging.join(".archive.part");
                    let digest = download_whole(&client, archive.url, &part).await?;
                    if digest != archive.sha256 {
                        return Err(digest_mismatch(archive.url, archive.sha256, &digest));
                    }
                    let (p, f) = (part.clone(), archive.files);
                    let read = tokio::task::spawn_blocking(move || extract_tar_gz(&p, f))
                        .await
                        .map_err(|e| e.to_string())??;
                    let _ = std::fs::remove_file(&part);
                    read
                }
            };
            for (name, bytes) in contents {
                let path = staging.join(&name);
                write_file(&path, &bytes)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
                }
                files.push(InstalledFile {
                    size: bytes.len() as u64,
                    sha256: hex(&bytes),
                    name,
                });
            }
            sources.push(InstalledSource {
                url: archive.url.to_string(),
                sha256: archive.sha256.to_string(),
            });
            base += archive.size;
            set_progress(|p| p.downloaded_bytes = base);
        }
        let manifest = Manifest {
            variant: spec.variant,
            version: spec.version.to_string(),
            files,
            sources,
            installed_at: crate::capabilities::now_secs(),
        };
        write_file(
            &staging.join("manifest.json"),
            &serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        )?;
        Ok::<(), String>(())
    }
    .await;
    if let Err(e) = outcome {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(e);
    }
    let _ = tokio::fs::remove_dir_all(&dir).await;
    {
        let (s, d) = (staging.clone(), dir.clone());
        tokio::task::spawn_blocking(move || crate::paths::rename_retrying(&s, &d))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))
            .and_then(|r| r)
    }
    .map_err(|e| {
        String::from(
            err("fs.rename_failed")
                .with("from", staging.display())
                .with("to", dir.display())
                .with("error", e),
        )
    })?;
    installed_in(root, spec.variant).ok_or_else(|| {
        String::from(err("onnx.runtime_missing").with("variant", spec.variant.as_str()))
    })
}

/// [`install_in`] in the data folder.
pub async fn install(variant: OrtVariant) -> Result<Installed, String> {
    let installed = install_in(&root(), variant).await?;
    // Also after the automatic install of a first ONNX launch: the Settings views reload.
    crate::events::publish(crate::events::CoreEvent::EnginesChanged);
    Ok(installed)
}

/// The library for a launch with `--provider <choice>`: installed, or an error that says to
/// install it.
pub fn lib_for_provider(choice: &str) -> Result<PathBuf, String> {
    let variant = variant_for_provider(choice);
    if !supported() {
        return Err(no_spec(variant));
    }
    installed(variant).map(|i| i.lib).ok_or_else(|| {
        err("onnx.runtime_missing")
            .with("variant", variant.as_str())
            .into()
    })
}

/// The GenAI library next to the ONNX Runtime of a launch with `--provider <choice>`:
/// `None` when that build is not installed or has no GenAI library (Linux, macOS).
pub fn genai_lib_for_provider(choice: &str) -> Option<PathBuf> {
    let lib = installed(variant_for_provider(choice))?.dir.join(GENAI_LIB);
    lib.is_file().then_some(lib)
}

/// Settings > Runtime view: every build of this platform with its state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VariantView {
    pub variant: OrtVariant,
    pub version: String,
    /// Download size of the pinned archives (the real transfer is smaller for ZIPs).
    pub download_bytes: u64,
    /// Size on disk once installed ([`VariantSpec::disk_bytes`]).
    pub disk_bytes: u64,
    pub default: bool,
    pub installed: Option<Installed>,
    /// Files of an install that are missing or changed in size ([`damaged_in`]); the build then
    /// reads as not installed and is installed again.
    #[serde(default)]
    pub damaged: Vec<String>,
}

pub fn view_in(root: &Path) -> Vec<VariantView> {
    specs()
        .iter()
        .map(|s| VariantView {
            variant: s.variant,
            version: s.version.to_string(),
            download_bytes: s.archives.iter().map(|a| a.size).sum(),
            disk_bytes: s.disk_bytes,
            default: s.variant == default_variant(),
            installed: installed_in(root, s.variant),
            damaged: damaged_in(root, s.variant),
        })
        .collect()
}

pub fn view() -> Vec<VariantView> {
    view_in(&root())
}

/// The sidecar binary: `HARNESS_ONNX_BIN`, else `harness-onnx` next to the running
/// executable (the Tauri bundle puts it there), else in the cargo target folder the
/// executable was built into (development and tests).
pub fn sidecar_binary() -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "harness-onnx.exe"
    } else {
        "harness-onnx"
    };
    if let Some(p) = std::env::var_os("HARNESS_ONNX_BIN").map(PathBuf::from) {
        return if p.is_file() {
            Ok(p)
        } else {
            Err(err("onnx.sidecar_missing").with("path", p.display()).into())
        };
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().unwrap_or(Path::new("."));
    // `target/debug/deps/<test>.exe` is two levels below the binaries.
    let candidates = [
        dir.join(name),
        dir.join("..").join(name),
        dir.join("binaries").join(name),
    ];
    for c in &candidates {
        if c.is_file() {
            return Ok(paths::normalize(c));
        }
    }
    Err(err("onnx.sidecar_missing")
        .with("path", candidates[0].display())
        .into())
}

#[cfg(test)]
mod tests;
