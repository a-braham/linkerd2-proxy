use futures::{try_ready, Future, Poll};
use http::header::AsHeaderName;
use linkerd2_stack::Make;
use std::marker::PhantomData;

/// Wraps HTTP `Service` `Stack<T>`s so that a given header is removed from a
/// request or response.
#[derive(Clone, Debug)]
pub struct Layer<H, R> {
    header: H,
    _marker: PhantomData<fn(R)>,
}

/// Wraps an HTTP `Service` so that a given header is removed from each
/// request or response.
#[derive(Clone, Debug)]
pub struct Stack<H, M, R> {
    header: H,
    inner: M,
    _marker: PhantomData<fn(R)>,
}

pub struct MakeFuture<H, F, R> {
    header: H,
    inner: F,
    _marker: PhantomData<fn(R)>,
}

#[derive(Clone, Debug)]
pub struct Service<H, S, R> {
    header: H,
    inner: S,
    _marker: PhantomData<fn(R)>,
}

// === impl Layer ===

/// Call `request::layer(header)` or `response::layer(header)`.
fn layer<H, R>(header: H) -> Layer<H, R>
where
    H: AsHeaderName + Clone,
    R: Clone,
{
    Layer {
        header,
        _marker: PhantomData,
    }
}

impl<H, M, R> tower::layer::Layer<M> for Layer<H, R>
where
    H: AsHeaderName + Clone,
{
    type Service = Stack<H, M, R>;

    fn layer(&self, inner: M) -> Self::Service {
        Stack {
            header: self.header.clone(),
            inner,
            _marker: PhantomData,
        }
    }
}

// === impl Stack ===

impl<H, T, M, R> Make<T> for Stack<H, M, R>
where
    H: AsHeaderName + Clone,
    M: Make<T>,
{
    type Service = Service<H, M::Service, R>;

    fn make(&self, t: T) -> Self::Service {
        let inner = self.inner.make(t);
        let header = self.header.clone();
        Service {
            header,
            inner,
            _marker: PhantomData,
        }
    }
}

// === impl MakeFuture ===

impl<H, F, R> Future for MakeFuture<H, F, R>
where
    H: Clone,
    F: Future,
{
    type Item = Service<H, F::Item, R>;
    type Error = F::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let inner = try_ready!(self.inner.poll());
        Ok(Service {
            header: self.header.clone(),
            inner,
            _marker: PhantomData,
        }
        .into())
    }
}

pub mod request {
    use futures::Poll;
    use http;
    use http::header::AsHeaderName;
    use linkerd2_stack::Proxy;

    pub fn layer<H>(header: H) -> super::Layer<H, ReqHeader>
    where
        H: AsHeaderName + Clone,
    {
        super::layer(header)
    }

    /// Marker type used to specify that the `Request` headers should be stripped.
    #[derive(Clone, Debug)]
    pub enum ReqHeader {}

    impl<H, P, S, B> Proxy<http::Request<B>, S> for super::Service<H, P, ReqHeader>
    where
        P: Proxy<http::Request<B>, S>,
        H: AsHeaderName + Clone,
        S: tower::Service<P::Request>,
    {
        type Request = P::Request;
        type Response = P::Response;
        type Error = P::Error;
        type Future = P::Future;

        fn proxy(&self, svc: &mut S, mut req: http::Request<B>) -> Self::Future {
            req.headers_mut().remove(self.header.clone());
            self.inner.proxy(svc, req)
        }
    }

    impl<H, S, B> tower::Service<http::Request<B>> for super::Service<H, S, ReqHeader>
    where
        H: AsHeaderName + Clone,
        S: tower::Service<http::Request<B>>,
    {
        type Response = S::Response;
        type Error = S::Error;
        type Future = S::Future;

        fn poll_ready(&mut self) -> Poll<(), Self::Error> {
            self.inner.poll_ready()
        }

        fn call(&mut self, mut req: http::Request<B>) -> Self::Future {
            req.headers_mut().remove(self.header.clone());
            self.inner.call(req)
        }
    }
}

pub mod response {
    use futures::{try_ready, Future, Poll};
    use http;
    use http::header::AsHeaderName;

    pub fn layer<H>(header: H) -> super::Layer<H, ResHeader>
    where
        H: AsHeaderName + Clone,
    {
        super::layer(header)
    }

    /// Marker type used to specify that the `Response` headers should be stripped.
    #[derive(Clone, Debug)]
    pub enum ResHeader {}

    pub struct ResponseFuture<F, H> {
        inner: F,
        header: H,
    }

    impl<H, S, B, Req> tower::Service<Req> for super::Service<H, S, ResHeader>
    where
        H: AsHeaderName + Clone,
        S: tower::Service<Req, Response = http::Response<B>>,
    {
        type Response = S::Response;
        type Error = S::Error;
        type Future = ResponseFuture<S::Future, H>;

        fn poll_ready(&mut self) -> Poll<(), Self::Error> {
            self.inner.poll_ready()
        }

        fn call(&mut self, req: Req) -> Self::Future {
            ResponseFuture {
                inner: self.inner.call(req),
                header: self.header.clone(),
            }
        }
    }

    impl<F, H, B> Future for ResponseFuture<F, H>
    where
        F: Future<Item = http::Response<B>>,
        H: AsHeaderName + Clone,
    {
        type Item = F::Item;
        type Error = F::Error;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            let mut res = try_ready!(self.inner.poll());
            res.headers_mut().remove(self.header.clone());
            Ok(res.into())
        }
    }
}
