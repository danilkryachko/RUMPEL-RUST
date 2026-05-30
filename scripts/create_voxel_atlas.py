import os
from PIL import Image

def main():
    assets_dir = "/Users/daniil/RUMPELRUST/assets/textures/blocks"
    output_path = os.path.join(assets_dir, "voxel_texture_array.png")

    # Define the exact vertical layout order of our 64x64 texture tiles (28 layers total)
    texture_files = [
        # Original RUMPEL blocks (0-7)
        "grass_block_top.png",         # 0: Grass Top
        "grass_block_side.png",        # 1: Grass Side
        "dirt.png",                    # 2: Dirt
        "stone.png",                   # 3: Stone
        "sand.png",                    # 4: Sand
        "wood_log_side.png",           # 5: Wood Log Side
        "wood_log_top.png",            # 6: Wood Log Top
        "leaves.png",                  # 7: Leaves
        
        # Clarity Ores (8-15)
        "coal_ore.png",                # 8: Coal Ore
        "iron_ore.png",                # 9: Iron Ore
        "copper_ore.png",              # 10: Copper Ore
        "gold_ore.png",                # 11: Gold Ore
        "diamond_ore.png",             # 12: Diamond Ore
        "emerald_ore.png",             # 13: Emerald Ore
        "lapis_ore.png",               # 14: Lapis Ore
        "redstone_ore.png",            # 15: Redstone Ore
        
        # Clarity Building Blocks (16-23)
        "cobblestone.png",             # 16: Cobblestone
        "stone_bricks.png",            # 17: Stone Bricks
        "bricks.png",                  # 18: Bricks
        "oak_planks.png",              # 19: Oak Planks
        "bookshelf.png",               # 20: Bookshelf
        "glass.png",                   # 21: Glass
        "obsidian.png",                # 22: Obsidian
        "glowstone.png",               # 23: Glowstone
        
        # Clarity Environmental Blocks (24-27)
        "snow.png",                    # 24: Snow
        "ice.png",                     # 25: Ice
        "gravel.png",                  # 26: Gravel
        "clay.png",                    # 27: Clay
    ]

    tile_size = 64
    total_height = tile_size * len(texture_files)

    # Create new blank vertical strip image with transparent background (RGBA)
    atlas = Image.new("RGBA", (tile_size, total_height), (0, 0, 0, 0))

    for idx, filename in enumerate(texture_files):
        img_path = os.path.join(assets_dir, filename)
        if not os.path.exists(img_path):
            raise FileNotFoundError(f"Required texture tile not found: {img_path}")
        
        tile_img = Image.open(img_path).convert("RGBA")
        if tile_img.size != (tile_size, tile_size):
            tile_img = tile_img.resize((tile_size, tile_size), Image.Resampling.LANCZOS)
            
        atlas.paste(tile_img, (0, idx * tile_size))
        print(f"Stitched {filename} at index {idx} (y={idx * tile_size})")

    # Save to path
    atlas.save(output_path, "PNG")
    print(f"\nSuccessfully generated 28-layer vertical texture atlas at: {output_path}")

if __name__ == "__main__":
    main()
