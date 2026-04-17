use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::{Map, Number, Value};

use crate::transpile::tla::{
    ContractBehaviorManifest, ContractBindingManifest, ContractExprManifest,
    ContractFixtureStepManifest, ContractManifest, ContractProjectionManifest,
};

#[derive(Debug, Clone, Default)]
pub struct ContractRunOptions {
    pub behavior: Option<String>,
    pub fixtures: Vec<String>,
    pub events: Vec<String>,
    pub projection: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContractRunReport {
    pub module_name: String,
    pub behavior: String,
    pub declared_behavior: String,
    pub applied_fixtures: Vec<String>,
    pub applied_events: Vec<String>,
    pub passed: bool,
    pub bindings: HashMap<String, Value>,
    pub tables: HashMap<String, Vec<Value>>,
    pub calls: Vec<ContractCallRecord>,
    pub expectations: Vec<ContractExpectationReport>,
    pub projections: Vec<ContractProjectionReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContractCallRecord {
    pub phase: String,
    pub path: String,
    pub args: HashMap<String, Value>,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContractProjectionReport {
    pub name: String,
    pub source: Option<String>,
    pub matched_rows: usize,
    pub states: Vec<String>,
    pub selected_state: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContractExpectationReport {
    pub event: String,
    pub condition: String,
    pub passed: bool,
    pub value: Value,
}

#[derive(Debug, Clone)]
struct RuntimeState {
    bindings: HashMap<String, Value>,
    tables: HashMap<String, Vec<Map<String, Value>>>,
    calls: Vec<ContractCallRecord>,
    next_id: i64,
    unique_counter: usize,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            bindings: HashMap::new(),
            tables: HashMap::new(),
            calls: Vec::new(),
            next_id: 1,
            unique_counter: 0,
        }
    }
}

fn render_contract_expr(expr: &ContractExprManifest) -> String {
    match expr {
        ContractExprManifest::Int { value } => value.to_string(),
        ContractExprManifest::Float { value } => value.to_string(),
        ContractExprManifest::DurationMs { value } => format!("{}ms", value),
        ContractExprManifest::String { value } => format!("{:?}", value),
        ContractExprManifest::Bool { value } => value.to_string(),
        ContractExprManifest::Null => "null".to_string(),
        ContractExprManifest::Ident { value } => value.clone(),
        ContractExprManifest::DottedName { value } => value.clone(),
        ContractExprManifest::Ref { name } => format!("${}", name),
        ContractExprManifest::Call { name, args } => format!(
            "{}({})",
            name,
            args.iter()
                .map(render_contract_expr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ContractExprManifest::List { items } => format!(
            "[{}]",
            items
                .iter()
                .map(render_contract_expr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ContractExprManifest::Object { fields } => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|field| format!("{}: {}", field.name, render_contract_expr(&field.value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ContractExprManifest::Binary { lhs, op, rhs } => format!(
            "({} {} {})",
            render_contract_expr(lhs),
            op,
            render_contract_expr(rhs)
        ),
        ContractExprManifest::Unary { op, expr } => {
            format!("({}{})", op, render_contract_expr(expr))
        }
        ContractExprManifest::Exists { source, filter } => match filter {
            Some(filter) => format!("exists({} where {})", source, render_contract_expr(filter)),
            None => format!("exists({})", source),
        },
    }
}

pub fn load_contract_manifest(path: &Path) -> Result<ContractManifest> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("reading contract manifest {}", path.display()))?;
    serde_json::from_str(&source)
        .with_context(|| format!("parsing contract manifest {}", path.display()))
}

pub fn run_contract_manifest_path(
    path: &Path,
    options: &ContractRunOptions,
) -> Result<ContractRunReport> {
    let manifest = load_contract_manifest(path)?;
    run_contract_manifest(&manifest, options)
}

pub fn run_contract_manifest(
    manifest: &ContractManifest,
    options: &ContractRunOptions,
) -> Result<ContractRunReport> {
    let behavior = select_behavior(manifest, options.behavior.as_deref())?;
    let fixture_filter: HashSet<&str> = options.fixtures.iter().map(String::as_str).collect();
    let event_filter: HashSet<&str> = options.events.iter().map(String::as_str).collect();

    let mut runtime = RuntimeState::default();
    let mut applied_fixtures = Vec::new();
    let mut applied_events = Vec::new();
    let mut expectations = Vec::new();
    let mut passed = true;

    for fixture in &behavior.fixtures {
        if !fixture_filter.is_empty() && !fixture_filter.contains(fixture.name.as_str()) {
            continue;
        }

        for step in &fixture.steps {
            runtime.apply_fixture_step(step)?;
        }
        applied_fixtures.push(fixture.name.clone());
    }

    for transition in &behavior.transition_bindings {
        if !event_filter.is_empty() && !event_filter.contains(transition.on_event.as_str()) {
            continue;
        }

        if transition.bindings.is_empty() && transition.expects.is_empty() {
            continue;
        }

        for binding in &transition.bindings {
            runtime.apply_transition_binding(binding)?;
        }

        for expect in &transition.expects {
            let value = runtime.eval_expr(expect, None)?;
            let expectation_passed = runtime.truthy(&value);
            if !expectation_passed {
                passed = false;
            }
            expectations.push(ContractExpectationReport {
                event: transition.on_event.clone(),
                condition: render_contract_expr(expect),
                passed: expectation_passed,
                value,
            });
        }
        applied_events.push(transition.on_event.clone());
    }

    let projections: Vec<_> = behavior
        .projections
        .iter()
        .filter(|projection| {
            options
                .projection
                .as_ref()
                .map(|name| name == &projection.name)
                .unwrap_or(true)
        })
        .map(|projection| runtime.evaluate_projection(projection))
        .collect::<Result<_>>()?;

    Ok(ContractRunReport {
        module_name: manifest.module_name.clone(),
        behavior: behavior.name.clone(),
        declared_behavior: behavior.declared_name.clone(),
        applied_fixtures,
        applied_events,
        passed,
        bindings: runtime.bindings,
        tables: runtime
            .tables
            .into_iter()
            .map(|(name, rows)| {
                let values = rows.into_iter().map(Value::Object).collect();
                (name, values)
            })
            .collect(),
        calls: runtime.calls,
        expectations,
        projections,
    })
}

fn select_behavior<'a>(
    manifest: &'a ContractManifest,
    requested: Option<&str>,
) -> Result<&'a ContractBehaviorManifest> {
    match requested {
        Some(name) => manifest
            .behaviors
            .iter()
            .find(|behavior| behavior.name == name || behavior.declared_name == name)
            .ok_or_else(|| anyhow!("unknown behavior '{}' in {}", name, manifest.module_name)),
        None if manifest.behaviors.len() == 1 => Ok(&manifest.behaviors[0]),
        None => Err(anyhow!(
            "manifest {} contains multiple behaviors; pass an explicit behavior name",
            manifest.module_name
        )),
    }
}

impl RuntimeState {
    fn apply_fixture_step(&mut self, step: &ContractFixtureStepManifest) -> Result<()> {
        match step {
            ContractFixtureStepManifest::Insert {
                target,
                fields,
                bind,
            } => {
                let mut row = Map::new();
                for field in fields {
                    row.insert(field.name.clone(), self.eval_expr(&field.value, None)?);
                }
                if !row.contains_key("id") {
                    row.insert("id".to_string(), Value::from(self.allocate_id()));
                }
                let bound_value = row
                    .get("id")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(row.clone()));
                self.tables.entry(target.clone()).or_default().push(row);
                if let Some(name) = bind {
                    self.bindings.insert(name.clone(), bound_value);
                }
            }
            ContractFixtureStepManifest::Call { path, args, bind } => {
                let evaluated_args = self.eval_named_args(args, None)?;
                let result = if bind.is_some() {
                    Value::from(self.allocate_id())
                } else {
                    self.synthetic_call_result(path, &evaluated_args)
                };
                self.calls.push(ContractCallRecord {
                    phase: "fixture".to_string(),
                    path: path.clone(),
                    args: evaluated_args.clone(),
                    result: result.clone(),
                });
                if let Some(name) = bind {
                    self.bindings.insert(name.clone(), result);
                } else {
                    self.bindings.insert("result".to_string(), result);
                }
            }
            ContractFixtureStepManifest::Bind { name, value } => {
                let evaluated = self.eval_expr(value, None)?;
                self.bindings.insert(name.clone(), evaluated);
            }
        }

        Ok(())
    }

    fn apply_transition_binding(&mut self, binding: &ContractBindingManifest) -> Result<()> {
        match binding {
            ContractBindingManifest::Call { path, args } => {
                let evaluated_args = self.eval_named_args(args, None)?;
                let result = self.synthetic_call_result(path, &evaluated_args);
                self.calls.push(ContractCallRecord {
                    phase: "transition".to_string(),
                    path: path.clone(),
                    args: evaluated_args,
                    result: result.clone(),
                });
                self.bindings.insert("result".to_string(), result);
            }
            ContractBindingManifest::Update {
                target,
                assignments,
                filter,
            } => {
                let existing = self.tables.remove(target).unwrap_or_default();
                let mut updated_rows = Vec::new();
                let mut matched = false;

                for mut row in existing {
                    let should_update = if let Some(expr) = filter.as_ref() {
                        let value = self.eval_expr(expr, Some(&row)).unwrap_or(Value::Null);
                        self.truthy(&value)
                    } else {
                        true
                    };
                    if should_update {
                        matched = true;
                        for assignment in assignments {
                            row.insert(
                                assignment.name.clone(),
                                self.eval_expr(&assignment.value, Some(&row))?,
                            );
                        }
                    }
                    updated_rows.push(row);
                }

                if !matched {
                    let mut row = self.seed_row_from_filter(filter)?;
                    for assignment in assignments {
                        row.insert(
                            assignment.name.clone(),
                            self.eval_expr(&assignment.value, Some(&row))?,
                        );
                    }
                    if !row.contains_key("id") {
                        row.insert("id".to_string(), Value::from(self.allocate_id()));
                    }
                    updated_rows.push(row);
                }

                self.tables.insert(target.clone(), updated_rows);
            }
        }

        Ok(())
    }

    fn evaluate_projection(
        &mut self,
        projection: &ContractProjectionManifest,
    ) -> Result<ContractProjectionReport> {
        let rows: Vec<Map<String, Value>> = if let Some(source) = &projection.source {
            let source_rows = self
                .tables
                .get(&source.source)
                .cloned()
                .unwrap_or_default();
            let mut filtered = Vec::new();
            for row in source_rows {
                let matches = if let Some(expr) = source.filter.as_ref() {
                    let value = self.eval_expr(expr, Some(&row)).unwrap_or(Value::Null);
                    self.truthy(&value)
                } else {
                    true
                };
                if matches {
                    filtered.push(row);
                }
            }
            filtered
        } else {
            vec![Map::new()]
        };

        let mut states = Vec::new();
        for row in &rows {
            let mut selected = projection.else_state.clone();
            for clause in &projection.clauses {
                let value = self.eval_expr(&clause.condition, Some(row))?;
                if self.truthy(&value) {
                    selected = Some(clause.state.clone());
                    break;
                }
            }
            if let Some(state) = selected {
                states.push(state);
            }
        }

        Ok(ContractProjectionReport {
            name: projection.name.clone(),
            source: projection.source.as_ref().map(|source| source.source.clone()),
            matched_rows: rows.len(),
            selected_state: states.first().cloned(),
            states,
        })
    }

    fn eval_named_args(
        &mut self,
        args: &[crate::transpile::tla::ContractNamedExpr],
        row: Option<&Map<String, Value>>,
    ) -> Result<HashMap<String, Value>> {
        let mut values = HashMap::new();
        for arg in args {
            values.insert(arg.name.clone(), self.eval_expr(&arg.value, row)?);
        }
        Ok(values)
    }

    fn eval_expr(
        &mut self,
        expr: &ContractExprManifest,
        row: Option<&Map<String, Value>>,
    ) -> Result<Value> {
        match expr {
            ContractExprManifest::Int { value } => Ok(Value::from(*value)),
            ContractExprManifest::Float { value } => {
                let number =
                    Number::from_f64(*value).ok_or_else(|| anyhow!("invalid float {}", value))?;
                Ok(Value::Number(number))
            }
            ContractExprManifest::DurationMs { value } => Ok(Value::from(*value)),
            ContractExprManifest::String { value } => Ok(Value::String(value.clone())),
            ContractExprManifest::Bool { value } => Ok(Value::Bool(*value)),
            ContractExprManifest::Null => Ok(Value::Null),
            ContractExprManifest::Ident { value } => Ok(self.resolve_ident(value, row)),
            ContractExprManifest::DottedName { value } => Ok(self.resolve_dotted(value, row)),
            ContractExprManifest::Ref { name } => Ok(self.bindings.get(name).cloned().unwrap_or(Value::Null)),
            ContractExprManifest::Call { name, args } => {
                let evaluated: Vec<Value> = args
                    .iter()
                    .map(|arg| self.eval_expr(arg, row))
                    .collect::<Result<_>>()?;
                Ok(self.eval_call(name, &evaluated))
            }
            ContractExprManifest::List { items } => Ok(Value::Array(
                items
                    .iter()
                    .map(|item| self.eval_expr(item, row))
                    .collect::<Result<_>>()?,
            )),
            ContractExprManifest::Object { fields } => Ok(Value::Object(
                fields
                    .iter()
                    .map(|field| {
                        Ok((field.name.clone(), self.eval_expr(&field.value, row)?))
                    })
                    .collect::<Result<_>>()?,
            )),
            ContractExprManifest::Binary { lhs, op, rhs } => {
                let lhs = self.eval_expr(lhs, row)?;
                let rhs = self.eval_expr(rhs, row)?;
                self.eval_binary(lhs, op, rhs)
            }
            ContractExprManifest::Unary { op, expr } => {
                let value = self.eval_expr(expr, row)?;
                match op.as_str() {
                    "not" => Ok(Value::Bool(!self.truthy(&value))),
                    "-" => self.numeric_binop(Value::from(0), value, |l, r| l - r),
                    other => Err(anyhow!("unsupported unary contract operator '{}'", other)),
                }
            }
            ContractExprManifest::Exists { source, filter } => {
                let rows = self.tables.get(source).cloned().unwrap_or_default();
                let mut matches = false;
                for candidate in rows {
                    let candidate_matches = if let Some(expr) = filter.as_ref() {
                        let value = self.eval_expr(expr, Some(&candidate)).unwrap_or(Value::Null);
                        self.truthy(&value)
                    } else {
                        true
                    };
                    if candidate_matches {
                        matches = true;
                        break;
                    }
                }
                Ok(Value::Bool(matches))
            }
        }
    }

    fn eval_call(&mut self, name: &str, args: &[Value]) -> Value {
        match name {
            "unique" => {
                self.unique_counter += 1;
                let prefix = args
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or("unique");
                Value::String(format!("{}-{}", prefix, self.unique_counter))
            }
            "uuid4" => {
                let id = self.allocate_id();
                Value::String(format!("uuid-{:08}", id))
            }
            "random_bytes" => {
                let width = args.first().and_then(Value::as_i64).unwrap_or(16).max(0) as usize;
                Value::String("00".repeat(width))
            }
            "db_pool" => Value::String("db_pool".to_string()),
            _ => {
                if args.iter().all(|value| value.is_number()) {
                    Value::String(format!("{}({})", name, args.len()))
                } else {
                    Value::String(format!("{}()", name))
                }
            }
        }
    }

    fn eval_binary(&self, lhs: Value, op: &str, rhs: Value) -> Result<Value> {
        match op {
            "+" => self.add_values(lhs, rhs),
            "-" => self.numeric_binop(lhs, rhs, |l, r| l - r),
            "*" => self.numeric_binop(lhs, rhs, |l, r| l * r),
            "/" => self.numeric_binop(lhs, rhs, |l, r| l / r),
            "=" => Ok(Value::Bool(lhs == rhs)),
            "!=" => Ok(Value::Bool(lhs != rhs)),
            "<" => Ok(Value::Bool(self.compare_values(&lhs, &rhs)? < 0)),
            "<=" => Ok(Value::Bool(self.compare_values(&lhs, &rhs)? <= 0)),
            ">" => Ok(Value::Bool(self.compare_values(&lhs, &rhs)? > 0)),
            ">=" => Ok(Value::Bool(self.compare_values(&lhs, &rhs)? >= 0)),
            "and" => Ok(Value::Bool(self.truthy(&lhs) && self.truthy(&rhs))),
            "or" => Ok(Value::Bool(self.truthy(&lhs) || self.truthy(&rhs))),
            other => Err(anyhow!("unsupported contract operator '{}'", other)),
        }
    }

    fn add_values(&self, lhs: Value, rhs: Value) -> Result<Value> {
        match (lhs, rhs) {
            (Value::String(lhs), Value::String(rhs)) => Ok(Value::String(lhs + &rhs)),
            (Value::String(lhs), rhs) => Ok(Value::String(lhs + &self.value_to_string(&rhs))),
            (lhs, Value::String(rhs)) => Ok(Value::String(self.value_to_string(&lhs) + &rhs)),
            (lhs, rhs) => self.numeric_binop(lhs, rhs, |l, r| l + r),
        }
    }

    fn numeric_binop<F>(&self, lhs: Value, rhs: Value, op: F) -> Result<Value>
    where
        F: FnOnce(f64, f64) -> f64,
    {
        let lhs = self.as_f64(&lhs)?;
        let rhs = self.as_f64(&rhs)?;
        let value = op(lhs, rhs);
        if value.fract() == 0.0 {
            Ok(Value::from(value as i64))
        } else {
            let number = Number::from_f64(value).ok_or_else(|| anyhow!("invalid numeric result"))?;
            Ok(Value::Number(number))
        }
    }

    fn compare_values(&self, lhs: &Value, rhs: &Value) -> Result<i8> {
        match (lhs, rhs) {
            (Value::String(lhs), Value::String(rhs)) => Ok(match lhs.cmp(rhs) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }),
            _ => {
                let lhs = self.as_f64(lhs)?;
                let rhs = self.as_f64(rhs)?;
                Ok(if lhs < rhs {
                    -1
                } else if lhs > rhs {
                    1
                } else {
                    0
                })
            }
        }
    }

    fn seed_row_from_filter(
        &mut self,
        filter: &Option<ContractExprManifest>,
    ) -> Result<Map<String, Value>> {
        let mut row = Map::new();
        if let Some(filter) = filter {
            self.collect_filter_bindings(filter, &mut row)?;
        }
        Ok(row)
    }

    fn collect_filter_bindings(
        &mut self,
        expr: &ContractExprManifest,
        row: &mut Map<String, Value>,
    ) -> Result<()> {
        match expr {
            ContractExprManifest::Binary { lhs, op, rhs } if op == "and" => {
                self.collect_filter_bindings(lhs, row)?;
                self.collect_filter_bindings(rhs, row)?;
            }
            ContractExprManifest::Binary { lhs, op, rhs } if op == "=" => {
                if let Some(field) = self.extract_field_name(lhs) {
                    row.insert(field, self.eval_expr(rhs, None)?);
                } else if let Some(field) = self.extract_field_name(rhs) {
                    row.insert(field, self.eval_expr(lhs, None)?);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn extract_field_name(&self, expr: &ContractExprManifest) -> Option<String> {
        match expr {
            ContractExprManifest::Ident { value } => Some(value.clone()),
            ContractExprManifest::DottedName { value } => value.rsplit('.').next().map(str::to_string),
            _ => None,
        }
    }

    fn resolve_ident(&self, name: &str, row: Option<&Map<String, Value>>) -> Value {
        row.and_then(|row| row.get(name).cloned())
            .or_else(|| self.bindings.get(name).cloned())
            .unwrap_or(Value::Null)
    }

    fn resolve_dotted(&self, path: &str, row: Option<&Map<String, Value>>) -> Value {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return Value::Null;
        }

        if matches!(parts[0], "meta" | "this") {
            return row
                .map(|row| self.lookup_path(&Value::Object(row.clone()), &parts[1..]))
                .unwrap_or(Value::Null);
        }

        if parts[0] == "memory" && parts.len() > 1 {
            return self
                .bindings
                .get(parts[1])
                .cloned()
                .unwrap_or(Value::Null);
        }

        if let Some(bound) = self.bindings.get(parts[0]) {
            return self.lookup_path(bound, &parts[1..]);
        }

        row.and_then(|row| row.get(parts[0]).cloned())
            .map(|value| self.lookup_path(&value, &parts[1..]))
            .unwrap_or(Value::Null)
    }

    fn lookup_path(&self, value: &Value, parts: &[&str]) -> Value {
        let mut current = value;
        for part in parts {
            match current {
                Value::Object(map) => match map.get(*part) {
                    Some(next) => current = next,
                    None => return Value::Null,
                },
                _ => return Value::Null,
            }
        }
        current.clone()
    }

    fn synthetic_call_result(&mut self, path: &str, args: &HashMap<String, Value>) -> Value {
        let call_id = self.allocate_id();
        let mut result = Map::new();
        result.insert("id".to_string(), Value::from(call_id));
        result.insert("ok".to_string(), Value::Bool(true));
        result.insert("path".to_string(), Value::String(path.to_string()));
        result.insert(
            "args".to_string(),
            Value::Object(args.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        );
        Value::Object(result)
    }

    fn allocate_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn truthy(&self, value: &Value) -> bool {
        match value {
            Value::Bool(value) => *value,
            Value::Null => false,
            Value::Number(number) => number
                .as_i64()
                .map(|value| value != 0)
                .or_else(|| number.as_u64().map(|value| value != 0))
                .or_else(|| number.as_f64().map(|value| value != 0.0))
                .unwrap_or(false),
            Value::String(value) => !value.is_empty(),
            Value::Array(values) => !values.is_empty(),
            Value::Object(values) => !values.is_empty(),
        }
    }

    fn as_f64(&self, value: &Value) -> Result<f64> {
        match value {
            Value::Number(number) => number
                .as_f64()
                .ok_or_else(|| anyhow!("number is not representable as f64")),
            Value::Bool(value) => Ok(if *value { 1.0 } else { 0.0 }),
            Value::String(value) => value
                .parse::<f64>()
                .with_context(|| format!("parsing '{}' as number", value)),
            other => Err(anyhow!("expected numeric value, found {}", other)),
        }
    }

    fn value_to_string(&self, value: &Value) -> String {
        match value {
            Value::String(value) => value.clone(),
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transpile::tla::{
        ContractBindingManifest, ContractBehaviorManifest, ContractExprManifest,
        ContractFixtureManifest, ContractFixtureStepManifest, ContractManifest,
        ContractMbtGeneratorManifest, ContractMbtManifest, ContractMbtReplayManifest,
        ContractNamedExpr, ContractProjectionClauseManifest, ContractProjectionManifest,
        ContractProjectionSourceManifest, ContractTransitionManifest,
    };

    fn sample_manifest() -> ContractManifest {
        ContractManifest {
            manifest_version: 1,
            module_name: "Sample_Flow".to_string(),
            behaviors: vec![ContractBehaviorManifest {
                name: "Flow".to_string(),
                declared_name: "Flow".to_string(),
                fixtures: vec![ContractFixtureManifest {
                    name: "seed_code".to_string(),
                    steps: vec![
                        ContractFixtureStepManifest::Insert {
                            target: "tenant".to_string(),
                            fields: vec![ContractNamedExpr {
                                name: "name".to_string(),
                                value: ContractExprManifest::String {
                                    value: "Tenant".to_string(),
                                },
                            }],
                            bind: Some("tenant_id".to_string()),
                        },
                        ContractFixtureStepManifest::Call {
                            path: "seed_code".to_string(),
                            args: vec![ContractNamedExpr {
                                name: "tenant_id".to_string(),
                                value: ContractExprManifest::Ref {
                                    name: "tenant_id".to_string(),
                                },
                            }],
                            bind: Some("code_id".to_string()),
                        },
                    ],
                }],
                projections: vec![ContractProjectionManifest {
                    name: "model_state".to_string(),
                    source: Some(ContractProjectionSourceManifest {
                        source: "db.authorization_code".to_string(),
                        filter: Some(ContractExprManifest::Binary {
                            lhs: Box::new(ContractExprManifest::Ident {
                                value: "id".to_string(),
                            }),
                            op: "=".to_string(),
                            rhs: Box::new(ContractExprManifest::Ref {
                                name: "code_id".to_string(),
                            }),
                        }),
                    }),
                    clauses: vec![ContractProjectionClauseManifest {
                        condition: ContractExprManifest::Binary {
                            lhs: Box::new(ContractExprManifest::DottedName {
                                value: "meta.used".to_string(),
                            }),
                            op: "=".to_string(),
                            rhs: Box::new(ContractExprManifest::Bool { value: true }),
                        },
                        state: "done".to_string(),
                    }],
                    else_state: Some("pending".to_string()),
                }],
                transition_bindings: vec![ContractTransitionManifest {
                    from: "pending".to_string(),
                    to: "done".to_string(),
                    on_event: "exchange".to_string(),
                    bindings: vec![
                        ContractBindingManifest::Call {
                            path: "svc::mark_used".to_string(),
                            args: vec![ContractNamedExpr {
                                name: "id".to_string(),
                                value: ContractExprManifest::Ref {
                                    name: "code_id".to_string(),
                                },
                            }],
                        },
                        ContractBindingManifest::Update {
                            target: "db.authorization_code".to_string(),
                            assignments: vec![
                                ContractNamedExpr {
                                    name: "used".to_string(),
                                    value: ContractExprManifest::Bool { value: true },
                                },
                                ContractNamedExpr {
                                    name: "tenant_id".to_string(),
                                    value: ContractExprManifest::Ref {
                                        name: "tenant_id".to_string(),
                                    },
                                },
                            ],
                            filter: Some(ContractExprManifest::Binary {
                                lhs: Box::new(ContractExprManifest::Ident {
                                    value: "id".to_string(),
                                }),
                                op: "=".to_string(),
                                rhs: Box::new(ContractExprManifest::Ref {
                                    name: "code_id".to_string(),
                                }),
                            }),
                        },
                    ],
                    expects: vec![
                        ContractExprManifest::Binary {
                            lhs: Box::new(ContractExprManifest::DottedName {
                                value: "result.ok".to_string(),
                            }),
                            op: "=".to_string(),
                            rhs: Box::new(ContractExprManifest::Bool { value: true }),
                        },
                        ContractExprManifest::Exists {
                            source: "db.authorization_code".to_string(),
                            filter: Some(Box::new(ContractExprManifest::Binary {
                                lhs: Box::new(ContractExprManifest::Ident {
                                    value: "used".to_string(),
                                }),
                                op: "=".to_string(),
                                rhs: Box::new(ContractExprManifest::Bool { value: true }),
                            })),
                        },
                    ],
                }],
                mbt: Some(ContractMbtManifest {
                    generator: Some(ContractMbtGeneratorManifest {
                        engine: "apalache".to_string(),
                        invariants: vec!["NotTerminated".to_string()],
                        max_traces: Some(8),
                        max_length: Some(3),
                        mode: Some("check".to_string()),
                        view: Some("state".to_string()),
                    }),
                    replay: Some(ContractMbtReplayManifest {
                        allow_unknown_action: Some(true),
                        state_projection: Some("model_state".to_string()),
                    }),
                }),
            }],
        }
    }

    #[test]
    fn runner_materializes_contract_manifest() {
        let report = run_contract_manifest(&sample_manifest(), &ContractRunOptions::default())
            .expect("runner should succeed");

        assert_eq!(report.behavior, "Flow");
        assert_eq!(report.applied_fixtures, vec!["seed_code"]);
        assert_eq!(report.applied_events, vec!["exchange"]);
        assert!(report.passed);
        assert_eq!(report.expectations.len(), 2);
        assert!(report.expectations.iter().all(|expectation| expectation.passed));
        assert_eq!(report.projections.len(), 1);
        assert_eq!(report.projections[0].selected_state.as_deref(), Some("done"));
        assert_eq!(report.tables["db.authorization_code"].len(), 1);
        assert_eq!(report.bindings["tenant_id"], Value::from(1));
        assert_eq!(report.bindings["code_id"], Value::from(2));
        assert_eq!(report.calls.len(), 2);
    }
}
