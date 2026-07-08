Scriptname MLChronicleQuest extends Quest
{Memory Layer chronicle. Polls player state on a real-time cadence and
 traces structured events to the "MemoryLayer" Papyrus user log
 (Logs/Script/User/MemoryLayer.0.log). The external bridge daemon tails
 that log and converts events into Memory Layer memories.

 Vanilla Papyrus only: no SKSE, no properties that require CK fill-ins.
 Location/worldspace names are emitted as debug strings, which retain
 editor IDs for LCTN/WRLD records; the bridge prettifies them.}

float Property PollSeconds = 10.0 Auto
{Real-time seconds between state polls.}

int Property GoldDeltaThreshold = 250 Auto
{Only chronicle gold swings at least this large since the last report.}

string lastLocation = ""
string lastWorldspace = ""
int lastLevel = 0
int lastGold = -1
bool wasInCombat = false
; GetCurrentRealTime() resets to ~0 every game launch, so seeing it go
; backwards is the vanilla-safe signal that a new play session started.
float lastRealTime = 999999.0

Event OnInit()
    ; Give the game a moment to finish loading the player into the world.
    RegisterForSingleUpdate(5.0)
EndEvent

Event OnUpdate()
    ; Reopening an already-open log is harmless; doing it every poll keeps
    ; tracing alive across save loads (OnInit only ever fires once).
    Debug.OpenUserLog("MemoryLayer")

    Actor player = Game.GetPlayer()
    if player == None || player.IsDead()
        RegisterForSingleUpdate(PollSeconds)
        return
    endif

    float realTime = Utility.GetCurrentRealTime()
    bool newSession = realTime < lastRealTime
    lastRealTime = realTime

    float day = Utility.GetCurrentGameTime()
    int level = player.GetLevel()
    string loc = "" + player.GetCurrentLocation()
    string world = "" + player.GetWorldSpace()
    int gold = player.GetItemCount(Game.GetForm(0xF))
    bool inCombat = player.IsInCombat()

    if newSession
        Trace("session|day=" + day + "|level=" + level + "|loc=" + loc + "|world=" + world + "|gold=" + gold)
        ; Re-baseline so a session start does not double-report as travel.
        lastLocation = loc
        lastWorldspace = world
        lastLevel = level
        lastGold = gold
        wasInCombat = inCombat
        RegisterForSingleUpdate(PollSeconds)
        return
    endif

    if level > lastLevel && lastLevel > 0
        Trace("level|level=" + level + "|loc=" + loc + "|day=" + day)
    endif
    lastLevel = level

    if loc != lastLocation
        Trace("location|from=" + lastLocation + "|to=" + loc + "|world=" + world + "|day=" + day + "|level=" + level)
        lastLocation = loc
        lastWorldspace = world
    endif

    if lastGold >= 0
        int delta = gold - lastGold
        if delta >= GoldDeltaThreshold || delta <= -GoldDeltaThreshold
            Trace("gold|gold=" + gold + "|delta=" + delta + "|loc=" + loc + "|day=" + day)
            lastGold = gold
        endif
    else
        lastGold = gold
    endif

    if inCombat && !wasInCombat
        Trace("combat|loc=" + loc + "|day=" + day + "|level=" + level)
    endif
    wasInCombat = inCombat

    RegisterForSingleUpdate(PollSeconds)
EndEvent

Function Trace(string msg)
    Debug.TraceUser("MemoryLayer", "ML1|" + msg)
EndFunction
