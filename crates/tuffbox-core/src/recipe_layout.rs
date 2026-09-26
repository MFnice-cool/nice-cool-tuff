//! JEI-style recipe layouts — crafting grid placement ported from
//! `CraftingGridHelper.getCraftingIndex` in the JEI reference repo.

use crate::unified::recipe::{parse_ingredient_value, parse_result_121, parse_result_value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngredientDisplay {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alts: Option<Vec<IngredientDisplay>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeLayout {
    pub category: String,
    pub shapeless: bool,
    /// Nine crafting slots (JEI 3×3), index 0 = top-left.
    pub grid: Vec<Option<IngredientDisplay>>,
    pub output: IngredientDisplay,
    pub output_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cook_time: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experience: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedRecipe {
    pub id: String,
    pub recipe_type: String,
    pub category: String,
    pub mod_source: String,
    pub source_file: String,
    pub layout: RecipeLayout,
    pub input_ids: Vec<String>,
    pub output_id: String,
    pub is_conditional: bool,
}

/// JEI `CraftingGridHelper.getCraftingIndex` — maps row-major recipe coords into 3×3 GUI.
pub fn get_crafting_index(i: usize, width: usize, height: usize) -> usize {
    let i = i as i32;
    let width = width as i32;
    let height = height as i32;
    let index = if width == 1 {
        if height == 3 || height == 2 {
            (i * 3) + 1
        } else {
            4
        }
    } else if height == 1 {
        i + 3
    } else if width == 2 {
        let mut index = i;
        if i > 1 {
            index += 1;
            if i > 3 {
                index += 1;
            }
        }
        index
    } else if height == 2 {
        i + 3
    } else {
        i
    };
    index.max(0) as usize
}

fn shapeless_size(total: usize) -> usize {
    if total > 4 {
        3
    } else if total > 1 {
        2
    } else {
        1
    }
}

fn ingredient_to_display(ing: &crate::unified::recipe::UnifiedIngredient) -> IngredientDisplay {
    use crate::unified::recipe::UnifiedIngredient;
    match ing {
        UnifiedIngredient::Item(id) => IngredientDisplay {
            id: id.clone(),
            kind: "item".into(),
            alts: None,
        },
        UnifiedIngredient::Tag(tag) => IngredientDisplay {
            id: if tag.starts_with('#') {
                tag.clone()
            } else {
                format!("#{tag}")
            },
            kind: "tag".into(),
            alts: None,
        },
        UnifiedIngredient::OneOf(alts) => {
            let displays: Vec<IngredientDisplay> = alts.iter().map(ingredient_to_display).collect();
            IngredientDisplay {
                id: displays.first().map(|d| d.id.clone()).unwrap_or_default(),
                kind: "one_of".into(),
                alts: Some(displays),
            }
        }
    }
}

/// Fill `alts` for tag ingredients from an offline [`TagIndex`] so the UI can
/// cycle member icons. Nested one_of entries are expanded recursively.
pub fn expand_layout_tags(layout: &mut RecipeLayout, tags: &crate::tag_index::TagIndex) {
    for slot in &mut layout.grid {
        if let Some(ing) = slot {
            expand_ingredient_tags(ing, tags);
        }
    }
    expand_ingredient_tags(&mut layout.output, tags);
}

fn expand_ingredient_tags(ing: &mut IngredientDisplay, tags: &crate::tag_index::TagIndex) {
    if let Some(alts) = ing.alts.as_mut() {
        for alt in alts.iter_mut() {
            expand_ingredient_tags(alt, tags);
        }
    }
    let is_tag = ing.kind == "tag" || ing.id.starts_with('#');
    if !is_tag {
        return;
    }
    if ing.alts.as_ref().is_some_and(|a| !a.is_empty()) {
        return;
    }
    let members = tags.expand_items(&ing.id);
    if members.is_empty() {
        return;
    }
    // Cap cycling set — huge tags (e.g. #minecraft:logs) would thrash the UI.
    const MAX_ALTS: usize = 64;
    ing.alts = Some(
        members
            .into_iter()
            .take(MAX_ALTS)
            .map(|id| IngredientDisplay {
                id,
                kind: "item".into(),
                alts: None,
            })
            .collect(),
    );
}

fn json_ingredient_to_display(value: &serde_json::Value) -> Option<IngredientDisplay> {
    parse_ingredient_value(value).map(|ing| ingredient_to_display(&ing))
}

fn empty_grid() -> Vec<Option<IngredientDisplay>> {
    vec![None; 9]
}

fn place_in_grid(
    grid: &mut [Option<IngredientDisplay>],
    flat: &[Option<IngredientDisplay>],
    width: usize,
    height: usize,
) {
    for (i, slot) in flat.iter().enumerate() {
        if slot.is_none() {
            continue;
        }
        let idx = get_crafting_index(i, width, height);
        if idx < 9 {
            grid[idx] = slot.clone();
        }
    }
}

pub fn category_for_type(recipe_type: &str) -> &'static str {
    match recipe_type {
        "minecraft:crafting_shaped" | "minecraft:crafting_shapeless" => "crafting",
        "minecraft:smelting"
        | "minecraft:blasting"
        | "minecraft:smoking"
        | "minecraft:campfire_cooking" => "cooking",
        "minecraft:smithing" | "minecraft:smithing_transform" | "minecraft:smithing_trim" => {
            "smithing"
        }
        "minecraft:stonecutting" => "stonecutting",
        t if t.contains("crafting") => "crafting",
        t if t.contains("smelt")
            || t.contains("blast")
            || t.contains("cook")
            || t.contains("furnace") =>
        {
            "cooking"
        }
        t if t.contains("smith") => "smithing",
        t if t.contains("stonecut") || t.contains("cutting") => "stonecutting",
        _ => "other",
    }
}

pub fn layout_from_json(json: &serde_json::Value, recipe_type: &str) -> RecipeLayout {
    let category = category_for_type(recipe_type).to_string();
    let output = parse_result_121(json)
        .or_else(|_| parse_result_value(json))
        .map(|o| IngredientDisplay {
            id: o.item.clone(),
            kind: "item".into(),
            alts: None,
        })
        .unwrap_or(IngredientDisplay {
            id: "unknown:unknown".into(),
            kind: "item".into(),
            alts: None,
        });
    let output_count = parse_result_121(json)
        .or_else(|_| parse_result_value(json))
        .map(|o| o.count)
        .unwrap_or(1);

    match recipe_type {
        "minecraft:crafting_shaped" => build_shaped_layout(json, category, output, output_count),
        "minecraft:crafting_shapeless" => {
            build_shapeless_layout(json, category, output, output_count)
        }
        "minecraft:smelting"
        | "minecraft:blasting"
        | "minecraft:smoking"
        | "minecraft:campfire_cooking" => {
            build_cooking_layout(json, category, output, output_count)
        }
        "minecraft:smithing_transform" | "minecraft:smithing" | "minecraft:smithing_trim" => {
            build_smithing_layout(json, category, output, output_count)
        }
        "minecraft:stonecutting" => build_stonecutting_layout(json, category, output, output_count),
        _ => build_generic_layout(json, category, output, output_count),
    }
}

fn build_shaped_layout(
    json: &serde_json::Value,
    category: String,
    output: IngredientDisplay,
    output_count: u32,
) -> RecipeLayout {
    let mut grid = empty_grid();
    let pattern = json
        .get("pattern")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let key = json.get("key").and_then(|k| k.as_object());
    let height = pattern.len();
    let width = pattern
        .iter()
        .filter_map(|row| row.as_str())
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(0);

    let mut flat: Vec<Option<IngredientDisplay>> = Vec::new();
    for row in &pattern {
        if let Some(row_str) = row.as_str() {
            for ch in row_str.chars() {
                if ch == ' ' {
                    flat.push(None);
                } else {
                    let sym = ch.to_string();
                    let ing = key
                        .and_then(|k| k.get(&sym))
                        .and_then(json_ingredient_to_display);
                    flat.push(ing);
                }
            }
        }
    }

    place_in_grid(&mut grid, &flat, width, height);

    RecipeLayout {
        category,
        shapeless: false,
        grid,
        output,
        output_count,
        cook_time: None,
        experience: None,
    }
}

fn build_shapeless_layout(
    json: &serde_json::Value,
    category: String,
    output: IngredientDisplay,
    output_count: u32,
) -> RecipeLayout {
    let ingredients: Vec<IngredientDisplay> = json
        .get("ingredients")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(json_ingredient_to_display).collect())
        .unwrap_or_default();

    let size = shapeless_size(ingredients.len());
    let mut flat: Vec<Option<IngredientDisplay>> = ingredients.into_iter().map(Some).collect();
    while flat.len() < size * size {
        flat.push(None);
    }

    let mut grid = empty_grid();
    place_in_grid(&mut grid, &flat, size, size);

    RecipeLayout {
        category,
        shapeless: true,
        grid,
        output,
        output_count,
        cook_time: None,
        experience: None,
    }
}

fn build_cooking_layout(
    json: &serde_json::Value,
    category: String,
    output: IngredientDisplay,
    output_count: u32,
) -> RecipeLayout {
    let input = json.get("ingredient").and_then(json_ingredient_to_display);
    let mut grid = empty_grid();
    grid[4] = input; // centered input like JEI furnace slot

    let cook_time = json
        .get("cookingtime")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let experience = json
        .get("experience")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);

    RecipeLayout {
        category,
        shapeless: false,
        grid,
        output,
        output_count,
        cook_time,
        experience,
    }
}

fn build_smithing_layout(
    json: &serde_json::Value,
    category: String,
    output: IngredientDisplay,
    output_count: u32,
) -> RecipeLayout {
    // JEI smithing: template | base | addition  (horizontal middle row)
    let mut grid = empty_grid();
    let keys = ["template", "base", "addition"];
    for (i, key) in keys.iter().enumerate() {
        if let Some(ing) = json.get(*key).and_then(json_ingredient_to_display) {
            grid[3 + i] = Some(ing);
        }
    }
    // Legacy smithing used base + addition only
    if grid[3].is_none() && grid[4].is_none() {
        if let Some(ing) = json.get("base").and_then(json_ingredient_to_display) {
            grid[3] = Some(ing);
        }
        if let Some(ing) = json.get("addition").and_then(json_ingredient_to_display) {
            grid[5] = Some(ing);
        }
    }
    RecipeLayout {
        category,
        shapeless: false,
        grid,
        output,
        output_count,
        cook_time: None,
        experience: None,
    }
}

fn build_stonecutting_layout(
    json: &serde_json::Value,
    category: String,
    output: IngredientDisplay,
    output_count: u32,
) -> RecipeLayout {
    let input = json.get("ingredient").and_then(json_ingredient_to_display);
    let mut grid = empty_grid();
    grid[4] = input;
    RecipeLayout {
        category,
        shapeless: false,
        grid,
        output,
        output_count,
        cook_time: None,
        experience: None,
    }
}

fn build_generic_layout(
    json: &serde_json::Value,
    category: String,
    output: IngredientDisplay,
    output_count: u32,
) -> RecipeLayout {
    let mut inputs = Vec::new();
    if let Some(ings) = json.get("ingredients").and_then(|v| v.as_array()) {
        for ing in ings {
            if let Some(d) = json_ingredient_to_display(ing) {
                inputs.push(d);
            }
        }
    }
    if inputs.is_empty() {
        if let Some(ing) = json.get("ingredient").and_then(json_ingredient_to_display) {
            inputs.push(ing);
        }
    }

    let size = shapeless_size(inputs.len().max(1));
    let flat: Vec<Option<IngredientDisplay>> = inputs.into_iter().map(Some).collect();
    let mut grid = empty_grid();
    place_in_grid(&mut grid, &flat, size, size);

    RecipeLayout {
        category,
        shapeless: true,
        grid,
        output,
        output_count,
        cook_time: None,
        experience: None,
    }
}

pub fn collect_item_ids(layout: &RecipeLayout) -> (Vec<String>, String) {
    let mut inputs = Vec::new();
    for slot in &layout.grid {
        if let Some(ing) = slot {
            collect_ingredient_ids(ing, &mut inputs);
        }
    }
    inputs.sort();
    inputs.dedup();
    (inputs, layout.output.id.clone())
}

fn collect_ingredient_ids(ing: &IngredientDisplay, out: &mut Vec<String>) {
    match ing.kind.as_str() {
        "one_of" | "tag" => {
            if let Some(alts) = &ing.alts {
                for alt in alts {
                    collect_ingredient_ids(alt, out);
                }
            }
            if ing.kind == "tag" || ing.id.starts_with('#') {
                out.push(ing.id.clone());
            }
        }
        _ => out.push(ing.id.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crafting_index_matches_jei_1x1() {
        assert_eq!(get_crafting_index(0, 1, 1), 4);
    }

    #[test]
    fn shaped_stick_pattern() {
        let json = serde_json::json!({
            "type": "minecraft:crafting_shaped",
            "pattern": ["XXX", " # ", " # "],
            "key": {
                "X": { "item": "minecraft:planks" },
                "#": { "item": "minecraft:stick" }
            },
            "result": { "item": "minecraft:wooden_pickaxe", "count": 1 }
        });
        let layout = layout_from_json(&json, "minecraft:crafting_shaped");
        assert!(!layout.shapeless);
        assert_eq!(layout.output.id, "minecraft:wooden_pickaxe");
        assert!(layout
            .grid
            .iter()
            .any(|s| s.as_ref().is_some_and(|i| i.id.contains("stick"))));
    }
}
