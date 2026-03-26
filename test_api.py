import urllib.request
import json
import os

with open('/run/media/deck/SteamDeck-SD/apps/gamer-sidekick/config.txt') as f:
    api_key = next((line.split('=')[1].strip() for line in f if 'STEAMGRIDDB_API_KEY' in line), None)

def req(ep):
    req = urllib.request.Request(f"https://www.steamgriddb.com/api/v2/{ep}", headers={"Authorization": f"Bearer {api_key}"})
    with urllib.request.urlopen(req) as response:
        return json.loads(response.read().decode("utf-8"))["data"]

hero = req('heroes/game/36615')
print("Hero:", hero[0]['url'] if hero else "none")
grid = req('grids/game/36615')
print("Grid (any):", grid[0]['url'] if grid else "none")
poster = req('grids/game/36615?dimensions=600x900')
print("Poster:", poster[0]['url'] if poster else "none")
