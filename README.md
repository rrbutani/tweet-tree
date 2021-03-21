# tweet-tree

a quick and dirty cli thing that'll walk a twitter thread

---

As an example: `cargo run -- 1373460240184836096 | dot -Tsvg -Gcharset=latin1 > out.svg` produces:

```plain
New User: ReductRs (@reduct_rs)
New User: Dr.Spaceman (@DrSpacemn)
New User: Jonas Schievink (@sheevink)
New User: Mihail Malostanidis (@qm3ster)
New User: maowtm (@WtmMao)
New User: foo (@foo78017145)
New User: Grumpy Fish 🌹🏴 (@jnsq)
New User: treelzebub (@treelzebub)
New User: gankra's gay (@Gankra_)
New User: Pangaea (@Pangaea__)
New User: C (@deanc)

13 tweets found! (11 unique users)
```

And then:
![An example graph from a tweet thread. Nodes are tweets, color coded to match their authors.](https://raw.githubusercontent.com/wiki/rrbutani/tweet-tree/example.svg)

