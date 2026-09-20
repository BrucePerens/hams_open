import numpy as np,sys
M=65536
t=np.fromfile('table_65536.i16','<i2').astype(float);t-=t.mean()
r=np.load('resid64.npy');r=r-r.mean()
def gen_v(a):
    v=np.zeros(M,dtype=np.int64);u=0
    for n in range(M):
        v[n]=u;u=(a*u+1)&0xFFFF
    return v
for a in (173,25381):
    v=gen_v(a)
    best=[]
    for tgt,name in ((r,'resid'),(t,'raw')):
        Ft=np.conj(np.fft.rfft(tgt));nt=np.sqrt((tgt**2).sum())
        top=(0,0,0)
        for c0 in range(1,M,2*128):
            cs=np.arange(c0,c0+256,2)
            o=((cs[:,None]*v[None,:])&0xFFFF).astype(np.int64)
            o=np.where(o>=32768,o-65536,o).astype(float)
            o-=o.mean(1,keepdims=True)
            cc=np.fft.irfft(np.fft.rfft(o,axis=1)*Ft[None,:],n=M,axis=1)/ (np.sqrt((o**2).sum(1))[:,None]*nt)
            i=np.unravel_index(np.argmax(abs(cc)),cc.shape)
            if abs(cc[i])>top[0]: top=(abs(cc[i]),int(cs[i[0]]),int(i[1]),cc[i])
        print(a,name,top,flush=True)
