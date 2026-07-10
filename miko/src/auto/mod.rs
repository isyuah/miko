mod route;
pub use route::*;

/// 创建一个由 inventory 注册项构成的独立依赖容器。
pub async fn init_container() -> std::sync::Arc<crate::dependency_container::LazyDependencyContainer>
{
    crate::dependency_container::default_container()
}
