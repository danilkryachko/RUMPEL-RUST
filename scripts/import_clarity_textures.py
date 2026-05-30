import os
import shutil

def main():
    src_dir = "/Users/daniil/Downloads/Clarity/assets/minecraft/textures/block"
    dest_dir = "/Users/daniil/RUMPELRUST/assets/textures/blocks"
    
    # Define a clean selection of the most useful Clarity block textures to import
    textures_to_import = [
        # Ores
        "coal_ore.png",
        "iron_ore.png",
        "copper_ore.png",
        "gold_ore.png",
        "diamond_ore.png",
        "emerald_ore.png",
        "lapis_ore.png",
        "redstone_ore.png",
        
        # Stonework / Bricks
        "cobblestone.png",
        "mossy_cobblestone.png",
        "stone_bricks.png",
        "mossy_stone_bricks.png",
        "cracked_stone_bricks.png",
        "chiseled_stone_bricks.png",
        "bricks.png",
        "obsidian.png",
        "crying_obsidian.png",
        "glowstone.png",
        "sea_lantern.png",
        
        # Planks
        "oak_planks.png",
        "spruce_planks.png",
        "birch_planks.png",
        "jungle_planks.png",
        "acacia_planks.png",
        "dark_oak_planks.png",
        "mangrove_planks.png",
        "cherry_planks.png",
        
        # Utility / Interactive
        "bookshelf.png",
        "glass.png",
        "crafting_table_top.png",
        "crafting_table_side.png",
        "crafting_table_front.png",
        "furnace_front.png",
        "furnace_side.png",
        "furnace_top.png",
        
        # Environmental
        "snow.png",
        "ice.png",
        "packed_ice.png",
        "blue_ice.png",
        "clay.png",
        "gravel.png",
        "red_sand.png",
        "podzol_top.png",
        "podzol_side.png",
        "mycelium_top.png",
        "mycelium_side.png",
    ]

    print(f"Importing block textures from Clarity pack...")
    imported_count = 0
    
    for filename in textures_to_import:
        src_path = os.path.join(src_dir, filename)
        dest_path = os.path.join(dest_dir, filename)
        
        if os.path.exists(src_path):
            shutil.copy2(src_path, dest_path)
            imported_count += 1
            print(f" -> Imported: {filename}")
        else:
            print(f" ! WARNING: Texture not found in Clarity source: {filename}")
            
    print(f"\nSuccessfully imported {imported_count} block textures from Clarity pack to {dest_dir}!")

if __name__ == "__main__":
    main()
