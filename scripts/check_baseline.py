"""Re-runnable hardware/software baseline check for this box.

Run:  mise exec -- python scripts/check_baseline.py
"""
from pathlib import Path
import shutil, subprocess, sys, time
import torch


def rule(t): print(f"\n\033[1m{t}\033[0m\n" + "-" * len(t))


rule("Software")
import transformers, peft, trl, bitsandbytes, accelerate, datasets
for m in (torch, transformers, peft, trl, bitsandbytes, accelerate, datasets):
    print(f"  {m.__name__:15s} {getattr(m, '__version__', '?')}")
print(f"  python          {sys.version.split()[0]}")

rule("GPU")
if not torch.cuda.is_available():
    sys.exit("CUDA not available")
p = torch.cuda.get_device_properties(0)
cc = f"sm_{p.major}{p.minor}"
free, total = torch.cuda.mem_get_info()
print(f"  {p.name}  {cc}  {total / 1024**3:.2f} GiB total")
print(f"  free now        {free / 1024**3:.2f} GiB  <-- this is your real budget")
print(f"  arch in build   {cc in torch.cuda.get_arch_list()}")

rule("Precision (decides the whole training config)")
native_bf16 = torch.cuda.is_bf16_supported(including_emulation=False)


def bench(dt):
    x = torch.randn(4096, 4096, device="cuda", dtype=dt)
    for _ in range(5):
        x @ x
    torch.cuda.synchronize()
    t = time.perf_counter()
    for _ in range(20):
        x @ x
    torch.cuda.synchronize()
    return 2 * 4096**3 / ((time.perf_counter() - t) / 20) / 1e12


for dt, n in ((torch.float16, "fp16"), (torch.bfloat16, "bf16"), (torch.float32, "fp32")):
    print(f"  {n}            {bench(dt):6.1f} TFLOP/s")
print(f"  native bf16     {native_bf16}")
if not native_bf16:
    print("  \033[33m=> USE fp16 + GradScaler. bf16 is emulated and ~6x slower.\033[0m")
print(f"  FlashAttn-2 ok  {p.major >= 8}  (needs sm_80+; use attn_implementation='sdpa')")

rule("4-bit / QLoRA")
from bitsandbytes.nn import Linear4bit
lin = Linear4bit(4096, 4096, bias=False, compute_dtype=torch.float16, quant_type="nf4").cuda()
lin(torch.randn(2, 128, 4096, device="cuda", dtype=torch.float16))
print("  NF4 forward     OK")
bitsandbytes.optim.PagedAdamW8bit([torch.nn.Parameter(torch.randn(4, device="cuda"))])
print("  PagedAdamW8bit  OK")

rule("Storage")
for path in (str(Path.home() / "Work"), "/"):
    u = shutil.disk_usage(path)
    print(f"  {path:16s} {u.free / 1024**3:6.0f} GiB free / {u.total / 1024**3:.0f} GiB")

rule("VRAM hogs (kill these before training)")
try:
    out = subprocess.run(
        ["nvidia-smi", "--query-compute-apps=pid,used_memory,process_name",
         "--format=csv,noheader"], capture_output=True, text=True, timeout=10).stdout.strip()
    print("  " + (out.replace("\n", "\n  ") if out else "none"))
except Exception as e:
    print(f"  n/a ({e})")
