use crate::app::config::ServerSettings;
use crate::dependency_container::{LazyDependencyContainer, default_container};
use crate::handler::Req;
use crate::http::convert::incoming_to_req::IncomingToInternal;
use crate::router::HttpSvc;
use crate::router::Router;
use hyper::Error as HyperError;
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder as AutoBuilder,
    service::TowerToHyperService,
};
use tokio::io::Result as IoResult;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing;

pub mod config;

/// 应用程序入口，负责持有配置与路由，并启动 HTTP 服务
pub struct Application {
    settings: ServerSettings,
    svc: HttpSvc<Req>,
    dependencies: std::sync::Arc<LazyDependencyContainer>,
}

/// 应用程序
impl Application {
    /// 使用给定的配置与 Router 构建一个应用实例
    pub fn new<S: Send + Sync + 'static>(settings: ServerSettings, router: Router<S>) -> Self {
        Self::with_dependencies(settings, router, default_container())
    }

    /// 使用显式依赖容器构建应用实例。
    pub fn with_dependencies<S: Send + Sync + 'static>(
        settings: ServerSettings,
        router: Router<S>,
        dependencies: std::sync::Arc<LazyDependencyContainer>,
    ) -> Self {
        Self {
            settings,
            svc: router.into_tower_service_with_container(dependencies.clone()),
            dependencies,
        }
    }

    /// 使用默认/合并后的配置与 Router 构建应用实例
    pub fn new_<S: Send + Sync + 'static>(router: Router<S>) -> Self {
        Self::new(ServerSettings::from_global_settings(), router)
    }

    pub fn dependencies(&self) -> &std::sync::Arc<LazyDependencyContainer> {
        &self.dependencies
    }

    /// 运行应用，基于配置中的地址与端口监听并处理请求
    ///
    /// 此方法会阻塞当前异步任务，直到出现网络错误或手动终止。
    pub async fn run(self) -> IoResult<()> {
        self.dependencies
            .prewarm_all()
            .await
            .map_err(std::io::Error::other)?;
        let addr = format!("{}:{}", self.settings.host, self.settings.port);
        let listener = TcpListener::bind(addr).await?;
        let executor = TokioExecutor::new();
        let service_handle = self.svc;
        // 创建任务跟踪器以管理连接生命周期
        let tracker = TaskTracker::new();
        // token
        let shutdown_token = CancellationToken::new();

        tracing::info!("listening on {}:{}", self.settings.host, self.settings.port);

        loop {
            tokio::select! {
                _ = shutdown_signal() => {
                    tracing::info!("shutdown signal received, terminating...");
                    shutdown_token.cancel();
                    break;
                }
                r = listener.accept() => {
                    let (stream, _) = match r {
                        Ok(pair) => pair,
                        Err(err) =>{
                            tracing::error!("failed to accept connection: {}", err);
                            continue;
                        }
                    };
                    let io = TokioIo::new(stream);

                    let service_with_conversion = IncomingToInternal {
                        inner: service_handle.clone(),
                    };
                    let hyper_service = TowerToHyperService::new(service_with_conversion);

                    let executor = executor.clone();
                    let shutdown_token = shutdown_token.clone();
                    tracker.spawn(async move {
                        let builder = AutoBuilder::new(executor);
                        let conn = builder.serve_connection_with_upgrades(io, hyper_service);
                        tokio::pin!(conn);
                        let res = tokio::select! {
                            r = conn.as_mut() => r,
                            _ = shutdown_token.cancelled() => {
                                conn.as_mut().graceful_shutdown();
                                conn.await
                            }
                        };
                        if let Err(err) = res {
                            if let Some(hyper_err) = err.downcast_ref::<HyperError>()
                                && hyper_err.is_incomplete_message() {
                                return;
                            }
                            tracing::warn!(error = ?err, "failed to serve connection");
                        }
                    });
                }
            }
        }
        // shutdown
        tracker.close();
        tracing::info!(
            "waiting for existing {} connections to close...",
            tracker.len()
        );
        let timeout = Duration::from_secs(30);
        match tokio::time::timeout(timeout, tracker.wait()).await {
            Ok(_) => {
                tracing::info!("all connections closed, shutdown complete.");
            }
            Err(_) => {
                tracing::warn!(
                    "timeout ({:?}) reached, forcing shutdown with {} active connections.",
                    timeout,
                    tracker.len()
                );
            }
        }
        Ok(())
    }
}

#[cfg(feature = "test")]
impl Application {
    pub fn test_client(&mut self) -> crate::test::test_client::TestClient {
        crate::test::test_client::TestClient::new(self.svc.clone())
    }
}

/// 监听终止信号
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
