package dev.tuffbox.jeibridge;

import com.google.gson.JsonArray;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import mezz.jei.api.runtime.IJeiRuntime;

import java.lang.reflect.Method;
import java.time.Instant;
import java.util.Collection;
import java.util.List;
import java.util.Optional;
import java.util.stream.Stream;

final class JeiSnapshotter {
    private static final int MAX_RECIPES = 8_000;

    private JeiSnapshotter() {
    }

    static JsonObject snapshot(IJeiRuntime runtime) {
        JsonObject root = new JsonObject();
        root.addProperty("source", "runtime");
        root.addProperty("generatedAt", Instant.now().toString());
        root.addProperty("protocolVersion", 1);
        JsonArray categoriesJson = new JsonArray();
        JsonArray recipesJson = new JsonArray();

        Object recipeManager = invoke(runtime, "getRecipeManager");
        Object ingredientManager = invoke(runtime, "getIngredientManager");
        Object helpers = invoke(runtime, "getJeiHelpers");
        Object focusFactory = invoke(helpers, "getFocusFactory");
        Object emptyFocus = invoke(focusFactory, "getEmptyFocusGroup");
        Stream<?> categories = stream(invoke(invoke(recipeManager, "createRecipeCategoryLookup"), "get"));

        int[] total = {0};
        categories.forEach(category -> {
            if (total[0] >= MAX_RECIPES) {
                return;
            }
            Object recipeType = invoke(category, "getRecipeType");
            String categoryId = stringify(firstPresent(
                tryInvoke(recipeType, "getUid"),
                tryInvoke(recipeType, "getIdentifier"),
                recipeType
            ));
            JsonObject categoryJson = new JsonObject();
            categoryJson.addProperty("id", categoryId);
            categoryJson.addProperty("title", componentText(invoke(category, "getTitle")));
            categoryJson.addProperty("width", integer(invoke(category, "getWidth"), 0));
            categoryJson.addProperty("height", integer(invoke(category, "getHeight"), 0));
            categoryJson.add("stations", craftingStations(recipeManager, ingredientManager, recipeType));
            categoriesJson.add(categoryJson);

            Object lookup = invoke(recipeManager, "createRecipeLookup", recipeType);
            stream(invoke(lookup, "get")).forEach(recipe -> {
                if (total[0]++ >= MAX_RECIPES) {
                    return;
                }
                recipesJson.add(recipeJson(
                    recipeManager,
                    ingredientManager,
                    category,
                    recipe,
                    emptyFocus,
                    categoryId
                ));
            });
        });

        root.add("categories", categoriesJson);
        root.add("recipes", recipesJson);
        root.addProperty("totalScanned", total[0]);
        root.addProperty("truncated", total[0] >= MAX_RECIPES);
        return root;
    }

    private static JsonObject recipeJson(
        Object recipeManager,
        Object ingredientManager,
        Object category,
        Object recipe,
        Object emptyFocus,
        String categoryId
    ) {
        JsonObject result = new JsonObject();
        Object recipeId = firstPresent(
            tryInvoke(category, "getRegistryName", recipe),
            tryInvoke(category, "getIdentifier", recipe),
            recipe.getClass().getName() + "@" + Integer.toHexString(System.identityHashCode(recipe))
        );
        String id = stringify(recipeId);
        result.addProperty("id", id);
        result.addProperty("recipeType", categoryId);
        result.addProperty("category", categoryId);
        result.addProperty("modSource", namespace(id));
        result.addProperty("sourceFile", "JEI runtime");
        result.addProperty("isConditional", false);

        JsonObject layout = new JsonObject();
        layout.addProperty("category", categoryId);
        layout.addProperty("shapeless", false);
        layout.add("grid", new JsonArray());
        layout.add("slots", new JsonArray());
        JsonArray inputs = new JsonArray();
        String[] outputId = {null};

        Object drawable = optionalValue(firstPresent(
            tryInvoke(recipeManager, "createRecipeLayoutDrawable", category, recipe, emptyFocus),
            tryInvoke(recipeManager, "createRecipeLayoutDrawableOrShowError", category, recipe, emptyFocus)
        ));
        if (drawable != null) {
            Object slotsView = invoke(drawable, "getRecipeSlotsView");
            Object slotViews = invoke(slotsView, "getSlotViews");
            if (slotViews instanceof Collection<?> slots) {
                JsonArray slotJson = new JsonArray();
                for (Object slot : slots) {
                    JsonObject serialized = slotJson(ingredientManager, slot);
                    slotJson.add(serialized);
                    String role = serialized.get("role").getAsString();
                    JsonArray alternatives = serialized.getAsJsonArray("ingredients");
                    if (!alternatives.isEmpty()) {
                        String ingredientId = alternatives.get(0).getAsJsonObject().get("id").getAsString();
                        if ("OUTPUT".equals(role)) {
                            if (outputId[0] == null) {
                                outputId[0] = ingredientId;
                                layout.add("output", alternatives.get(0));
                                layout.addProperty("outputCount",
                                    alternatives.get(0).getAsJsonObject().get("count").getAsLong());
                            }
                        } else if ("INPUT".equals(role)) {
                            inputs.add(ingredientId);
                        }
                    }
                }
                layout.add("slots", slotJson);
            }
        }
        if (!layout.has("output")) {
            JsonObject unknown = new JsonObject();
            unknown.addProperty("id", "runtime:unknown");
            unknown.addProperty("name", "Unknown output");
            unknown.addProperty("count", 1);
            unknown.add("tooltip", new JsonArray());
            layout.add("output", unknown);
            layout.addProperty("outputCount", 1);
            outputId[0] = "runtime:unknown";
        }
        result.add("inputIds", inputs);
        if (outputId[0] == null) {
            result.add("outputId", JsonNull.INSTANCE);
        } else {
            result.addProperty("outputId", outputId[0]);
        }
        result.add("layout", layout);
        return result;
    }

    private static JsonObject slotJson(Object ingredientManager, Object slot) {
        JsonObject result = new JsonObject();
        result.addProperty("role", stringify(invoke(slot, "getRole")));
        result.addProperty("name", stringify(optionalValue(tryInvoke(slot, "getSlotName"))));
        Object area = firstPresent(
            tryInvoke(slot, "getAreaIncludingBackground"),
            tryInvoke(slot, "getArea")
        );
        result.addProperty("x", integer(firstPresent(tryInvoke(area, "getX"), tryInvoke(area, "x")), 0));
        result.addProperty("y", integer(firstPresent(tryInvoke(area, "getY"), tryInvoke(area, "y")), 0));
        result.addProperty("width", integer(firstPresent(tryInvoke(area, "getWidth"), tryInvoke(area, "width")), 18));
        result.addProperty("height", integer(firstPresent(tryInvoke(area, "getHeight"), tryInvoke(area, "height")), 18));

        JsonArray ingredients = new JsonArray();
        stream(invoke(slot, "getAllIngredients")).forEach(typed ->
            ingredients.add(ingredientJson(ingredientManager, typed)));
        result.add("ingredients", ingredients);
        return result;
    }

    private static JsonObject ingredientJson(Object ingredientManager, Object typed) {
        Object ingredient = invoke(typed, "getIngredient");
        Object helper = invoke(ingredientManager, "getIngredientHelper", ingredient);
        String id = stringify(firstPresent(
            tryInvoke(helper, "getIdentifier", ingredient),
            tryInvoke(helper, "getUid", ingredient, enumConstant("mezz.jei.api.ingredients.UidContext", "Ingredient")),
            ingredient
        ));
        JsonObject result = new JsonObject();
        result.addProperty("id", id);
        result.addProperty("name", stringify(firstPresent(
            tryInvoke(helper, "getDisplayName", ingredient),
            ingredient
        )));
        result.addProperty("count", longValue(tryInvoke(helper, "getAmount", ingredient), 1));
        JsonArray tooltip = new JsonArray();
        tooltip.add(result.get("name").getAsString());
        result.add("tooltip", tooltip);
        return result;
    }

    private static JsonArray craftingStations(Object recipeManager, Object ingredientManager, Object recipeType) {
        Object lookup = firstPresent(
            tryInvoke(recipeManager, "createCraftingStationLookup", recipeType),
            tryInvoke(recipeManager, "createRecipeCatalystLookup", recipeType)
        );
        JsonArray result = new JsonArray();
        if (lookup != null) {
            Object values = firstPresent(tryInvoke(lookup, "get"), lookup);
            stream(values).forEach(typed -> result.add(ingredientJson(ingredientManager, typed)));
        }
        return result;
    }

    private static Object invoke(Object target, String name, Object... args) {
        Object value = tryInvoke(target, name, args);
        if (value == null) {
            throw new IllegalStateException("JEI API method unavailable: " + name);
        }
        return value;
    }

    private static Object tryInvoke(Object target, String name, Object... args) {
        if (target == null) {
            return null;
        }
        for (Method method : target.getClass().getMethods()) {
            if (!method.getName().equals(name) || method.getParameterCount() != args.length) {
                continue;
            }
            try {
                method.setAccessible(true);
                return method.invoke(target, args);
            } catch (ReflectiveOperationException | RuntimeException ignored) {
            }
        }
        return null;
    }

    private static Stream<?> stream(Object value) {
        value = optionalValue(value);
        if (value instanceof Stream<?> stream) return stream;
        if (value instanceof Collection<?> collection) return collection.stream();
        if (value == null) return Stream.empty();
        return Stream.of(value);
    }

    private static Object optionalValue(Object value) {
        return value instanceof Optional<?> optional ? optional.orElse(null) : value;
    }

    private static Object firstPresent(Object... values) {
        for (Object value : values) {
            value = optionalValue(value);
            if (value != null) return value;
        }
        return null;
    }

    private static Object enumConstant(String className, String name) {
        try {
            @SuppressWarnings("unchecked")
            Class<? extends Enum> type = (Class<? extends Enum>) Class.forName(className);
            return Enum.valueOf(type, name);
        } catch (ReflectiveOperationException | IllegalArgumentException ignored) {
            return null;
        }
    }

    private static String componentText(Object component) {
        Object text = tryInvoke(component, "getString");
        return stringify(text == null ? component : text);
    }

    private static int integer(Object value, int fallback) {
        return value instanceof Number number ? number.intValue() : fallback;
    }

    private static long longValue(Object value, long fallback) {
        return value instanceof Number number && number.longValue() >= 0 ? number.longValue() : fallback;
    }

    private static String stringify(Object value) {
        return value == null ? "" : value.toString();
    }

    private static String namespace(String id) {
        int colon = id.indexOf(':');
        return colon > 0 ? id.substring(0, colon) : "runtime";
    }
}
