// The Python engine (python-hf): parameter catalog labels (params_pyhf.toml). Its error keys
// are in errorsRuntime.ts.
const tr = {
  'pyparam.threads': 'İş parçacığı',
  'pyparam.threads_help': 'PyTorch’un kullanacağı CPU iş parçacığı sayısı (boş = otomatik).',
  'pyparam.device': 'Aygıt',
  'pyparam.device_help': 'Modelin çalışacağı aygıt: auto (varsa CUDA veya Apple MPS, yoksa CPU), cpu, cuda veya mps. Kurulu CPU ortamında yalnız CPU vardır.',
  'pyparam.dtype': 'Sayı türü',
  'pyparam.dtype_help': 'Ağırlıkların yükleneceği tür: auto (modelin kendi türü), float32, float16 veya bfloat16. CPU’da float32 en güvenlisidir.',
};

const en: Record<keyof typeof tr, string> = {
  'pyparam.threads': 'Threads',
  'pyparam.threads_help': 'Number of CPU threads PyTorch uses (empty = automatic).',
  'pyparam.device': 'Device',
  'pyparam.device_help': 'Where the model runs: auto (CUDA or Apple MPS when present, else the CPU), cpu, cuda or mps. The installed CPU environment only has the CPU.',
  'pyparam.dtype': 'Number type',
  'pyparam.dtype_help': 'The type weights load in: auto (the model’s own), float32, float16 or bfloat16. float32 is the safest on a CPU.',
};

export default { tr, en };
