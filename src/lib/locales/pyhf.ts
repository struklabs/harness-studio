// The Python engine (python-hf): parameter catalog labels (params_pyhf.toml). Its error keys
// are in errorsRuntime.ts.
const tr = {
  'pyparam.threads': 'İş parçacığı',
  'pyparam.threads_help': 'PyTorch’un kullanacağı CPU iş parçacığı sayısı (boş = otomatik).',
  'pyparam.device': 'Aygıt',
  'pyparam.device_help': 'Modelin çalışacağı aygıt: auto (varsa CUDA veya Apple MPS, yoksa CPU), cpu, cuda veya mps. Kurulu CPU ortamında yalnız CPU vardır.',
  'pyparam.dtype': 'Sayı türü',
  'pyparam.dtype_help': 'Ağırlıkların yükleneceği tür: auto (modelin kendi türü), float32, float16 veya bfloat16. CPU’da float32 en güvenlisidir.',
  'pyparam.max_megapixels': 'En büyük görsel',
  'pyparam.max_megapixels_help': 'Görsel üretiminde bir isteğin en çok kaç megapiksel isteyebileceği (genişlik × yükseklik × görsel sayısı). Daha büyük istekler reddedilir; bellek taşmasını önler. Boş = CPU’da 1, GPU’da 4.',
  'pyparam.low_memory': 'Az bellek',
  'pyparam.low_memory_help': 'Görsel üretiminde dikkati dilimler hâlinde, VAE’yi parça parça çalıştırır: tepe bellek düşer, süre biraz uzar. auto = CPU’da ve Apple GPU’sunda açık.',
};

const en: Record<keyof typeof tr, string> = {
  'pyparam.threads': 'Threads',
  'pyparam.threads_help': 'Number of CPU threads PyTorch uses (empty = automatic).',
  'pyparam.device': 'Device',
  'pyparam.device_help': 'Where the model runs: auto (CUDA or Apple MPS when present, else the CPU), cpu, cuda or mps. The installed CPU environment only has the CPU.',
  'pyparam.dtype': 'Number type',
  'pyparam.dtype_help': 'The type weights load in: auto (the model’s own), float32, float16 or bfloat16. float32 is the safest on a CPU.',
  'pyparam.max_megapixels': 'Largest image',
  'pyparam.max_megapixels_help': 'Image generation: the most megapixels one request may ask for (width × height × images). Larger requests are refused, so memory does not run out. Empty = 1 on the CPU, 4 on a GPU.',
  'pyparam.low_memory': 'Low memory',
  'pyparam.low_memory_help': 'Image generation: attention runs in slices and the VAE a part at a time, for a lower memory peak and slightly longer runs. auto = on for the CPU and Apple’s GPU.',
};

export default { tr, en };
