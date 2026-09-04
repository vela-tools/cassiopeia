use serde_json::Value;

/// Recursively merges `src` into `dst`.
///
/// For objects, keys from `src` are added to `dst` without overwriting existing
/// keys, except nested objects, which merge recursively. Non-object conflicts
/// keep the existing value. Only brand-new keys pay a clone: existing keys are
/// looked up by reference.
pub(crate) fn deep_merge(dst: &mut Value, src: &Value) {
    if let (Value::Object(dst_map), Value::Object(src_map)) = (dst, src) {
        for (key, src_val) in src_map {
            if let Some(existing) = dst_map.get_mut(key.as_str()) {
                deep_merge(existing, src_val);
            } else {
                dst_map.insert(key.clone(), src_val.clone());
            }
        }
    }
}
