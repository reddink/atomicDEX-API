use super::{CollectCursorAction, CollectItemAction, CursorError, CursorOps, CursorResult, DbFilter};
use async_trait::async_trait;
use common::{serialize_to_js, stringify_js_error};
use js_sys::Array;
use mm2_err_handle::prelude::*;
use serde_json::Value as Json;
use wasm_bindgen::prelude::*;
use web_sys::{IdbIndex, IdbKeyRange};

/// The representation of a range that includes records
/// whose fields have only the specified [`IdbSingleCursor::only_values`] values.
/// https://developer.mozilla.org/en-US/docs/Web/API/IDBKeyRange/only
pub struct IdbMultiKeyCursor {
    only_values: Vec<(String, Json)>,
}

impl IdbMultiKeyCursor {
    pub(super) fn new(only_values: Vec<(String, Json)>) -> CursorResult<IdbMultiKeyCursor> {
        Self::check_only_values(&only_values)?;
        Ok(IdbMultiKeyCursor { only_values })
    }

    fn check_only_values(only_values: &Vec<(String, Json)>) -> CursorResult<()> {
        if only_values.len() < 2 {
            let description = format!(
                "Incorrect usage of 'IdbMultiKeyCursor': expected more than one cursor bound, found '{}'",
                only_values.len(),
            );
            return MmError::err(CursorError::IncorrectUsage { description });
        }
        Ok(())
    }
}

#[async_trait(?Send)]
impl CursorOps for IdbMultiKeyCursor {
    fn key_range(&self) -> CursorResult<Option<IdbKeyRange>> {
        let only = Array::new();

        for (field, value) in self.only_values.iter() {
            let js_value = serialize_to_js(value).map_to_mm(|e| CursorError::ErrorSerializingIndexFieldValue {
                field: field.to_owned(),
                value: format!("{:?}", value),
                description: e.to_string(),
            })?;
            only.push(&js_value);
        }

        let key_range = IdbKeyRange::only(&only).map_to_mm(|e| CursorError::InvalidKeyRange {
            description: stringify_js_error(&e),
        })?;
        Ok(Some(key_range))
    }

    fn on_collect_iter(
        &mut self,
        _key: JsValue,
        value: &Json,
    ) -> CursorResult<(CollectItemAction, CollectCursorAction)> {
        Ok((CollectItemAction::Include, CollectCursorAction::Continue))
    }
}
