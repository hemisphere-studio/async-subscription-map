<h1 align="center">async-subscription-map</h1>
<div align="center">
  <strong>
    Async bookkeeping datastructure for state subscriptions
  </strong>
</div>
<br />
<div align="center">
  <a href="https://crates.io/crates/async-subscription-map">
    <img src="https://img.shields.io/crates/v/async-subscription-map.svg?style=flat-square"
    alt="crates.io version" />
  </a>
  <a href="https://docs.rs/async-subscription-map">
    <img src="https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square"
      alt="docs.rs docs" />
  </a>
</div>
<br />

A subscription map is a self cleaning map of `Observable`s which tracks tasks
that want to subscribe to state updates on a certain key. This becomes very
useful if you have multiple tasks in your program and you want to wait in one
task until the other starts publishing state updates.

It enables you to generically  communicate through your whole program by just
knowing an identifier, no need to pass observables around - they are created on
the fly and only if someone subscribes to them. This is ideal for highly
asynchronous and performance critical backend implementations which serve data
accross multiple channels and want to cut down latency through communicating in
memory.

![Usage Diagram](./docs/diagram.png?raw=true "Diagram")

## Self Cleaning Nature

The subscription map is selfcleaing in the sense that it removes every
subscription entry and its data as soon as no one subscribes to it and thus
actively preventing memory leaks!

## Observables

This project is build ontop of
[async-observable](https://crates.io/crates/async-observable), take a look at
it to understand the underlying synchronization api.

---

<div align="center">
  <h6>Ackowledgement</h6>
</div>

This code was originally published by the [HUM
Systems](https://github.com/Hum-Systems). This repository continues the
development of this library as they sadly stopped their open source efforts.
