// The Python engine (python-hf): parameter catalog labels (params_pyhf.toml) and the backend
// error keys of its launches.
const tr = {
  'pyparam.threads': 'İş parçacığı',
  'pyparam.threads_help': 'PyTorch’un kullanacağı CPU iş parçacığı sayısı (boş = otomatik).',
  'pyparam.device': 'Aygıt',
  'pyparam.device_help': 'Modelin çalışacağı aygıt: auto (varsa CUDA veya Apple MPS, yoksa CPU), cpu, cuda veya mps. Kurulu CPU ortamında yalnız CPU vardır.',
  'pyparam.dtype': 'Sayı türü',
  'pyparam.dtype_help': 'Ağırlıkların yükleneceği tür: auto (modelin kendi türü), float32, float16 veya bfloat16. CPU’da float32 en güvenlisidir.',
  'process.pyhf_loopback_only': 'Python motoru yalnız 127.0.0.1 / localhost üzerinde dinler ({host}). Dışarıya ağ geçidi üzerinden açın.',
  'pyhf.model_unsupported': 'Bu klasör Python motorunun çalıştırabileceği bir model değil ({path}): görevi bilinmiyor, ağırlığı yok ya da uzaktan kod istiyor.',
};

const en: Record<keyof typeof tr, string> = {
  'pyparam.threads': 'Threads',
  'pyparam.threads_help': 'Number of CPU threads PyTorch uses (empty = automatic).',
  'pyparam.device': 'Device',
  'pyparam.device_help': 'Where the model runs: auto (CUDA or Apple MPS when present, else the CPU), cpu, cuda or mps. The installed CPU environment only has the CPU.',
  'pyparam.dtype': 'Number type',
  'pyparam.dtype_help': 'The type weights load in: auto (the model’s own), float32, float16 or bfloat16. float32 is the safest on a CPU.',
  'process.pyhf_loopback_only': 'The Python runtime only listens on 127.0.0.1 / localhost ({host}). Expose it through the gateway.',
  'pyhf.model_unsupported': 'This folder is not a model the Python runtime can run ({path}): its task is unknown, it has no weights, or it asks for remote code.',
};

export default { tr, en };
