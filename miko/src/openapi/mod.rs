#![cfg(all(feature = "utoipa", feature = "auto"))]

use utoipa::Modify;

use crate::utoipa;

pub struct OpenApiRoute {
    pub openapi: fn() -> utoipa::openapi::OpenApi,
    pub module_path: &'static str,
}

inventory::collect!(OpenApiRoute);

pub struct AutoPaths;

impl Modify for AutoPaths {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        for route in inventory::iter::<OpenApiRoute> {
            let mut route_openapi = (route.openapi)();
            apply_default_tag(&mut route_openapi, route.module_path);
            openapi.merge(route_openapi);
        }
    }
}

fn apply_default_tag(openapi: &mut utoipa::openapi::OpenApi, module_path: &str) {
    if module_path.is_empty() {
        return;
    }

    for path_item in openapi.paths.paths.values_mut() {
        set_default_tag(path_item.get.as_mut(), module_path);
        set_default_tag(path_item.put.as_mut(), module_path);
        set_default_tag(path_item.post.as_mut(), module_path);
        set_default_tag(path_item.delete.as_mut(), module_path);
        set_default_tag(path_item.options.as_mut(), module_path);
        set_default_tag(path_item.head.as_mut(), module_path);
        set_default_tag(path_item.patch.as_mut(), module_path);
        set_default_tag(path_item.trace.as_mut(), module_path);
    }
}

fn set_default_tag(operation: Option<&mut utoipa::openapi::path::Operation>, module_path: &str) {
    let Some(operation) = operation else {
        return;
    };

    if operation.tags.as_ref().is_none_or(Vec::is_empty) {
        operation.tags = Some(vec![module_path.to_string()]);
    }
}
