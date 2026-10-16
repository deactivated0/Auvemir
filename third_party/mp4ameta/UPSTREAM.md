# Vendored MP4 chapter reader

This directory vendors the `mp4ameta` 0.13.0 source from
<https://github.com/saecki/mp4ameta/tree/v0.13.0> (commit
`2878c96b9b662df348ed70efc9a45618b750f4da`). The crate is licensed under either
MIT or Apache-2.0; both upstream license texts are included here.

The only local source change is a crate-level allowance for `unused_parens`, a
warning emitted by newer Rust compilers for upstream's generic parameter syntax.
It has no runtime effect.
