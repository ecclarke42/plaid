# _"They've gone to plaid!"_ [📺](https://www.youtube.com/watch?v=mk7VWcuVOf0)

[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

## Archived

Use [Axum](https://github.com/tokio-rs/axum) instead! It does just about everything I set out to do with this (especially with the next release, which should include a radix-tree based router), except maybe the client-generation (which is a bit brittle anyway).


<!-- ![logo](./plaid/plaid2.jpg) -->
<div style="height:10em;width:100%;background-image:url(./plaid2.jpg);background-position: center;" />
<br/><br/><br/><br/></br></br></br>

## Plaid is a (WIP) Rust web framework built on top of [`hyper`](https://github.com/hyperium/hyper/) and [`tokio`](https://github.com/tokio-rs/tokio), with included [`tracing`](https://github.com/tokio-rs/tracing) support

<!-- TODO: Describe plaid -->

## Installation

## Usage

### Server

Plaid's `Server` wraps the `hyper` server. At it's core, it hanldes requests by passing them to a `Router`, but you can also define a array of `Middleware`s to "wrap" around the behaviour of the router (much like ExpressJS). Some sensible defualt middlewares (like `Cors` are provided).

<!-- TODO: No Cors -->

The server also needs functions to handle 404 and 5XX status responses. Naive defaults are provided (returning the status and an empty body for each), but you can also define your own functions. The server also allows you to define a custom error type that your handler will return. When an error is returned instead of a response, your error handler will translate that into a response body.

#### Context

Context is important. Most web servers have some references that every handler needs access to (e.g. a database connection pool). Plaid makes this easy out of the box. Supply an initial `Context` value to the server at start up and an `Arc<Context>` clone will be passed to every handler when it is called.

---

### Router

Plaid comes with a built-in router that uses a split implementation of the Adaptive Radix Tree to parse routes lightning fast [TODO: Benchmarks]. The router supports route parameters using the `/:name` syntax. It also supports parsing routes as multiple types, which can be hinted by following the name by a curly bracketed type name (e.g. `/:id{i32}`). Each handler is called with a `RouteParameters` struct that contains the parsed route parameters (a `Parameter` enum holding the corresponding values), both ordered and by name.

The default type for parameters is `String`. If a parameter cannot be parsed to the specified type, the router will report that the route was not found.

For example, given the route `/hello/:name/:id{i32}`, the following routes would process as:

```txt
/hello/yourname/123      -> FOUND     {name: "yourname", id: 123}
/hello/yourname/a_string -> NOT FOUND
/hello/123/123           -> FOUND     {name: "123", id: 123}
```

WIP: Support for unnamed route parameters (`/*`)

Not WIP: Wildcard parameters (`/some*`, which matches `/something`, `/someone`, etc.). Does anyone actually use this pattern? If so, let me know and I'll reconsider.

---

### Handlers

Plaid encourages decoupling the http part of writing a server from the application logic.

---

### Middleware

---
