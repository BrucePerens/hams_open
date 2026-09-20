#!/usr/bin/env python3
"""analyze.py <outdir>: JMBE vs our decoders. Envelope corr, band levels, error-concealment stats."""
import sys, numpy as np, scipy.signal as ss, warnings
warnings.filterwarnings("ignore")
D = sys.argv[1]
rd = lambda p: np.fromfile(p, dtype='<i2').astype(float)
def env(x, hop=160):
    n = len(x)//hop; return 10*np.log10((x[:n*hop].reshape(n,hop)**2).mean(1)+1)
def best_corr(a,b,hop=40,maxlag=4):
    ea,eb=env(a,hop),env(b,hop); best=(-2,0)
    for L in range(-maxlag,maxlag+1):
        x,y=(ea[L:],eb[:len(eb)-L]) if L>=0 else (ea[:L],eb[-L:])
        m=min(len(x),len(y)); c=np.corrcoef(x[:m],y[:m])[0,1]
        if c>best[0]: best=(c,L)
    return best
def bands(x):
    f,p=ss.welch(x,8000,nperseg=512); e=[10*np.log10(p[(f>=lo)&(f<hi)].sum()+1e-9) for lo,hi in zip(range(0,4000,500),range(500,4500,500))]
    return np.array(e)
def frame_stats(ref,o):
    dr,do=env(ref),env(o); act=dr>30
    exc=np.maximum(do-dr-3,0)
    return dict(dist=np.abs(do-dr)[act].mean(), exc=exc.mean(), worst=np.sort(exc)[int(len(exc)*.99)], muted=100*(do[act]<dr[act]-25).mean())
inp=np.fromfile("../../tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav",dtype="<i2",offset=44).astype(float)[:128000]
print(f"input RMS (800 frames) = {np.sqrt((inp**2).mean()):.0f}")
for m in ("imbe","a2"):
    print(f"=== {m} ===")
    ours=rd(f"{D}/{m}.ours.clean.raw"); j=rd(f"{D}/{m}.jmbe.clean.raw")
    if m=="a2":
        try: j=rd(f"{D}/a2.jmbe.clean.raw")
        except Exception: pass
    n=min(len(ours),len(j)); ours,j=ours[:n],j[:n]
    c20=np.corrcoef(env(ours),env(j))[0,1]; c10,lag=best_corr(ours,j)
    print(f"clean: env corr (20ms, lag0)={c20:.4f}; 10ms-hop best={c10:.4f} at lag {lag}; RMS ours={np.sqrt((ours**2).mean()):.0f} jmbe={np.sqrt((j**2).mean()):.0f} ({20*np.log10(np.sqrt((ours**2).mean())/np.sqrt((j**2).mean())):+.2f} dB)")
    bo,bj=bands(ours),bands(j)
    print("band (Hz)  :"," ".join(f"{lo:>5}-{lo+500:<5}" for lo in range(0,4000,500)))
    print("ours-JMBE dB:"," ".join(f"{d:>11.2f}" for d in bo-bj))
    print(f"  ours vs input env corr {np.corrcoef(env(ours),env(inp[:n]))[0,1]:.3f}, JMBE vs input {np.corrcoef(env(j),env(inp[:n]))[0,1]:.3f}")
    # per-frame level difference distribution
    de=env(ours)-env(j); act=(env(j)>30)
    print(f"per-frame level diff (ours-JMBE, active): mean {de[act].mean():+.2f} dB, sd {de[act].std():.2f}, worst |{np.abs(de[act]).max():.1f}| at frame {np.argmax(np.where(act,np.abs(de),0))}")
    # errors
    je=rd(f"{D}/{m}.jmbe.err.raw")[:n]
    print("2% BER (each vs its OWN clean decode): dist dB / mean excess dB / worst1% excess / muted active %")
    cands=[("JMBE",je,j)]
    for nm in ([f"{m}.ours.err.raw",f"{m}.ours.errfade.raw"] if m=="imbe" else [f"{m}.ours.err.raw",f"{m}.ours.errclean.raw"]):
        cands.append((nm.split('.',1)[1][:-4],rd(f"{D}/{nm}")[:n],ours))
    for nm,o,cl in cands:
        s=frame_stats(cl,o[:n]); print(f"  {nm:10s} {s['dist']:.2f} / {s['exc']:.2f} / {s['worst']:.1f} / {s['muted']:.1f}%")
    print(f"  JMBE-err vs ours-err env corr: {np.corrcoef(env(je),env(rd(f'{D}/{m}.ours.err.raw')[:n]))[0,1]:.4f}")
