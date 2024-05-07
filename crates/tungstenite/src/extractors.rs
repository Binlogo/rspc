use rspc::ExecError;
use tungstenite::http::request::Parts;

pub trait TCtxFunc<TCtx, TMarker>: Clone + Send + Sync + 'static {
    fn exec<'req>(&self, parts: Parts) -> Result<TCtx, ExecError>
    where
        TCtx: Send + 'req;
}
