//! The §4.4 check: after the merge, every `prod-required` value is set and no
//! `dev-only` value is enabled when the root's deployment is prod. Only the
//! class rule lives here; what a class MEANS for a specific value is the
//! field's own `#[config(...)]` attribute.

use crate::schema::{Class, Field, Kind};
use crate::values::Values;
use crate::{Deployment, Refusal};

pub(crate) fn enforce(values: &Values, deployment: Deployment) -> Result<(), Refusal> {
    if deployment != Deployment::Prod {
        return Ok(());
    }
    for field in values.fields() {
        match field.class {
            Class::Neutral => {}
            Class::ProdRequired => {
                if values.get(&field.path).is_none() {
                    return Err(refusal(field, values.app(), "unset")
                        .with_detail("prod-required and unset under DEPLOYMENT_TYPE=prod"));
                }
            }
            Class::DevOnly => {
                if enabled(values, field)? {
                    let text = values.get(&field.path).map_or("", |e| &e.text);
                    return Err(refusal(field, values.app(), text)
                        .with_detail("dev-only and set under DEPLOYMENT_TYPE=prod"));
                }
            }
        }
    }
    Ok(())
}

fn enabled(values: &Values, field: &Field) -> Result<bool, Refusal> {
    match field.kind {
        Kind::Bool => Ok(values.bool_opt(&field.path)?.unwrap_or(false)),
        Kind::Text => Ok(values.get(&field.path).is_some()),
    }
}

fn refusal(field: &Field, app: &str, value: &str) -> Refusal {
    let shown = if field.secret && value != "unset" {
        crate::values::MASK
    } else {
        value
    };
    Refusal::new(
        field.env(app),
        shown,
        match field.class {
            Class::ProdRequired => format!(
                "a value under DEPLOYMENT_TYPE=prod ({})",
                spellings(field, app)
            ),
            _ => format!(
                "unset under DEPLOYMENT_TYPE=prod ({})",
                spellings(field, app)
            ),
        },
    )
}

fn spellings(field: &Field, app: &str) -> String {
    format!("{}, {}, {}", field.env(app), field.flag(), field.toml())
}
