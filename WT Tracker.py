"""
WT Tracker - War Thunder Enemy Tracker
Отслеживает врагов в бою через War Thunder API и распознаёт нации по флагам
"""
import sys
import os
import json
import re
import time
from typing import Optional, Dict, Set, List, Tuple

import requests
from PyQt6.QtWidgets import (
    QApplication, QWidget, QVBoxLayout, QHBoxLayout,
    QCheckBox, QLabel, QPushButton, QGroupBox, QSlider,
    QFrame, QGridLayout, QMessageBox, QLineEdit, QComboBox
)
from PyQt6.QtCore import Qt, QTimer, pyqtSignal, QUrl, QThread
from PyQt6.QtMultimedia import QMediaPlayer, QAudioOutput
from PIL import Image

# Подавление лишних логов Qt
os.environ["QT_LOGGING_RULES"] = "qt.qpa.window=false;qt.multimedia.ffmpeg=false"

# Проверка доступности OpenCV (для точного распознавания флагов)
try:
    import cv2
    import numpy as np
    CV2_AVAILABLE = True
except ImportError:
    cv2 = None
    np = None
    CV2_AVAILABLE = False
    print("[WARN] OpenCV не найден. Будет использован fallback на pyautogui (точность ниже).")

# Проверка pyautogui (для скриншотов)
try:
    import pyautogui
    pyautogui.FAILSAFE = False
except ImportError:
    pyautogui = None
    print("[ERROR] pyautogui не установлен! Сканирование флагов недоступно.")

# Проверка keyboard (для глобальных хоткеев)
try:
    import keyboard
except ImportError:
    keyboard = None
    print("[ERROR] keyboard не установлен! Хоткеи недоступны.")

# ==================== КОНСТАНТЫ ====================

def get_base_dir() -> str:
    """Получить базовую директорию (для .exe или .py)"""
    if getattr(sys, 'frozen', False):
        return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.abspath(__file__))


BASE_DIR = get_base_dir()
VEHICLES_FILE = os.path.join(BASE_DIR, "vehicles.json")
UNKNOWN_FILE = os.path.join(BASE_DIR, "unknown.txt")
CONFIG_FILE = os.path.join(BASE_DIR, "config.json")
FLAGS_DIR = os.path.join(BASE_DIR, "flags")
SOUND_ENEMY = os.path.join(BASE_DIR, "alert.mp3")
SOUND_ALLY = os.path.join(BASE_DIR, "alert_ally.mp3")
API_URL = "http://localhost:8111"

# Пороги для детекции ядерного удара
NUKE_KILLS_THRESHOLD = 9
NUKE_DEATHS_MAX = 2

# Маппинг названий наций (для UI)
NATION_DISPLAY_NAMES = {
    "USA": "США",
    "Germany": "Германия",
    "USSR": "СССР",
    "Great Britain": "Британия",
    "Japan": "Япония",
    "China": "Китай",
    "Italy": "Италия",
    "France": "Франция",
    "Sweden": "Швеция",
    "Israel": "Израиль"
}

# Иконки типов техники
VEHICLE_TYPE_ICONS = {
    "tank": "🛡️ ",
    "plane": "✈️ ",
    "heli": "🚁 ",
    "zsu": "📡 ",
    "special": "⭐ "
}


# ==================== КЛАССЫ ДАННЫХ ====================


class PlayerStats:
    """Статистика игрока в текущем бою"""
    __slots__ = ('name', 'kills', 'deaths', 'is_disqualified', 'alert_played')
    
    def __init__(self, name: str):
        self.name = name
        self.kills = 0
        self.deaths = 0
        self.is_disqualified = False  # Дисквалификация за использование авиации
        self.alert_played = False  # Флаг проигрывания звука ядерки
    
    @property
    def can_nuke(self) -> bool:
        """Может ли игрок вызвать ядерку"""
        return (self.kills >= NUKE_KILLS_THRESHOLD and 
                self.deaths <= NUKE_DEATHS_MAX and 
                not self.is_disqualified)


class WTLogic:
    """Основная бизнес-логика трекера"""
    
    # Техника, доступная обеим сторонам (клоны)
    SHARED_VEHICLES = {
        "mq-1", "▄m55", "clovis", "df105", "m44", "▄m44",
        "nasams 3 (соу)", "nasams 3 (соц)"
    }
    
    # Паттерны для исключения из анализа (снаряды, ракеты и т.д.)
    GARBAGE_PATTERNS = [
        "weapons/", "us_hellfire ", "agm_", "tow ", "short ",
        "aim_", "mim_", "kh_", "rocket", "bomb ", "torpedo ",
        "su_9m", "m8_hvap", "m82_shot", "su_r_73", "us_iris_t_sl", "fb10_"
    ]
    
    # Ключевые слова действий в логе
    KILL_ACTIONS = ["уничтожил", "сбил", "destroyed", "shot down"]
    SOLO_ACTIONS = ["разбился", "выведен из строя", "самоуничтожился", "crashed"]
    
    def __init__(self, sound_callback):
        self.sound_callback = sound_callback
        
        # API и состояние игры
        self.session = requests.Session()
        self.is_api_available = False
        self.is_in_battle = False
        self.skip_history_mode = True
        self.last_dmg_id = 0
        
        # Данные игроков и техники
        self.active_enemies: Dict[str, dict] = {}
        self.dead_enemies: Dict[str, dict] = {}
        self.all_players: Dict[str, PlayerStats] = {}
        
        # База данных техники
        self.vehicle_db: Dict[str, dict] = {}
        self.name_cache: Dict[str, Optional[dict]] = {}
        self.unknown_buffer: Set[str] = set()
        
        # Фильтры
        self.selected_nations: Set[str] = set()
        self.selected_types: Set[str] = set()
        self.show_notes = True
        self.show_names = True

    def load_db(self) -> bool:
        """Загрузить базу данных техники из JSON"""
        if not os.path.exists(VEHICLES_FILE):
            print(f"[WARN] База данных не найдена: {VEHICLES_FILE}")
            return False
        try:
            with open(VEHICLES_FILE, "r", encoding="utf-8-sig") as f:
                self.vehicle_db = json.load(f)
            print(f"[OK] Загружено {len(self.vehicle_db)} записей техники")
            return True
        except Exception as e:
            print(f"[ERROR] Ошибка загрузки БД: {e}")
            return False

    def save_unknowns(self):
        """Сохранить неизвестную технику в файл для последующего добавления"""
        if not self.unknown_buffer:
            return
        
        try:
            # Читаем существующие записи
            existing = set()
            if os.path.exists(UNKNOWN_FILE):
                with open(UNKNOWN_FILE, "r", encoding="utf-8") as f:
                    existing = {line.strip() for line in f 
                               if line.strip() and not line.startswith("//")}
            
            # Объединяем и сохраняем
            to_write = existing.union(self.unknown_buffer)
            with open(UNKNOWN_FILE, "w", encoding="utf-8") as f:
                f.write("// Неизвестная техника для добавления в vehicles.json\n")
                f.write("// Формат: 'Название': {'type': 'tank', 'nation': 'USA', 'note': 'Заметка'}\n\n")
                for name in sorted(to_write):
                    f.write(f"{name}\n")
            
            print(f"[OK] Сохранено {len(to_write)} неизвестных записей")
            self.unknown_buffer.clear()
        except Exception as e:
            print(f"[ERROR] Ошибка сохранения unknown.txt: {e}")

    @staticmethod
    def normalize_name(name: Optional[str]) -> str:
        """Нормализовать название для поиска"""
        return (name or "").strip().lower()

    def is_garbage(self, name: str) -> bool:
        """Проверить, является ли название техники мусором (снаряд, ракета и т.д.)"""
        name_lower = name.lower()
        
        # Калибры (напр. "105mm_cannon")
        if re.match(r'^\d+mm_', name_lower):
            return True
        
        # Ракеты серии 9М (напр. "9m114")
        if re.match(r'^9m\d+', name_lower):
            return True
        
        # Разведывательные дроны
        if "recon micro" in name_lower:
            return True
        
        # Список шаблонов мусора
        return "/" in name_lower or any(pattern in name_lower 
                                        for pattern in self.GARBAGE_PATTERNS)

    def find_vehicle_info(self, log_name: Optional[str]) -> Optional[dict]:
        """Найти информацию о технике в базе данных"""
        if not log_name:
            return None
        
        # Проверка кеша
        if log_name in self.name_cache:
            return self.name_cache[log_name]
        
        # Проверка на мусор
        if self.is_garbage(log_name):
            self.name_cache[log_name] = None
            return None
        
        # Клон-техника (доступна обеим сторонам)
        clean_name_lower = log_name.lower()
        for shared_name in self.SHARED_VEHICLES:
            if shared_name in clean_name_lower:
                v_type = "aircraft" if "mq-1" in clean_name_lower else "tank"
                note_text = "Ударный БПЛА" if "mq-1" in clean_name_lower else "Клон-техника"
                result = {"type": v_type, "nation": "unknown", "note": note_text}
                self.name_cache[log_name] = result
                return result
        
        # Прямое совпадение по ключу
        if log_name in self.vehicle_db:
            result = self.vehicle_db[log_name]
            self.name_cache[log_name] = result
            return result
        
        # Нечеткий поиск (case-insensitive)
        clean_log = self.normalize_name(log_name)
        for db_name, info in self.vehicle_db.items():
            if clean_log == self.normalize_name(db_name):
                self.name_cache[log_name] = info
                return info
        
        # Не найдено - добавляем в список неизвестных
        self.unknown_buffer.add(log_name)
        self.name_cache[log_name] = None
        return None

    def get_player(self, name: str) -> PlayerStats:
        """Получить статистику игрока (создать если не существует)"""
        if name not in self.all_players:
            self.all_players[name] = PlayerStats(name)
        return self.all_players[name]

    @staticmethod
    def is_air_vehicle(v_type: str) -> bool:
        """Проверить, является ли техника авиацией"""
        return v_type in ('plane', 'heli', 'jet_fighter')

    def parse_entity(self, text: Optional[str]) -> Tuple[Optional[str], Optional[str]]:
        """
        Парсинг записи лога "Игрок (Техника)"
        
        Возвращает: (имя_игрока, название_техники)
        Обрабатывает особый случай ИИ-систем: "CLAWS (СОУ)" -> ("AI_System", "CLAWS (СОУ)")
        """
        if not text:
            return None, None
        
        match = re.match(r'^(.+?)\s\((.+)\)$', text.strip())
        if not match:
            return None, None
        
        player_name = match.group(1).strip()
        vehicle_name = match.group(2).strip()
        
        # Особый случай: если техника определилась ТОЛЬКО как "СОУ" или "СОЦ",
        # значит это ИИ-система без имени игрока
        if vehicle_name.upper() in ("СОУ", "СОЦ"):
            return "AI_System", text.strip()
        
        return player_name, vehicle_name

    def reset_session_data(self):
        """Сброс данных при начале новой сессии"""
        self.active_enemies.clear()
        self.dead_enemies.clear()
        self.all_players.clear()
        self.name_cache.clear()
        self.skip_history_mode = True
        print("[INFO] >>> НОВАЯ СЕССИЯ: Данные сброшены")

    def check_battle_status(self):
        """Проверить статус боя через API"""
        try:
            r = self.session.get(f"{API_URL}/mission.json", timeout=0.5)
            
            if r.status_code != 200:
                if self.is_in_battle:
                    self.reset_session_data()
                self.is_api_available = False
                self.is_in_battle = False
                return
            
            self.is_api_available = True
            data = r.json()
            status = data.get("status", "")
            currently_in_battle = (status == "running")
            
            # Переход в бой
            if currently_in_battle and not self.is_in_battle:
                self.reset_session_data()
            
            # Выход из боя
            if not currently_in_battle and self.is_in_battle:
                self.reset_session_data()
            
            self.is_in_battle = currently_in_battle
            
        except Exception:
            if self.is_in_battle:
                self.reset_session_data()
            self.is_api_available = False
            self.is_in_battle = False

    def _process_kill_event(self, text: str) -> bool:
        """Обработать событие убийства. Возвращает True если событие обработано"""
        action_found = None
        for act in self.KILL_ACTIONS:
            if act in text:
                action_found = act
                break
        
        if not action_found:
            return False
        
        parts = text.split(action_found, 1)
        if len(parts) != 2:
            return False
        
        # Парсинг убийцы и жертвы
        killer_raw = parts[0].strip()
        victim_raw = parts[1].strip()
        
        killer_name, killer_veh = self.parse_entity(killer_raw)
        victim_name, victim_veh = self.parse_entity(victim_raw)
        
        # Обработка дронов без имени игрока
        if not victim_veh:
            victim_veh = victim_raw
            victim_name = "AI_Drone"
        
        if not killer_name:
            return False
        
        # Убираем временные метки из имен
        killer_name = re.sub(r'^\d+:\d+', '', killer_name).strip()
        if victim_name != "AI_Drone":
            victim_name = re.sub(r'^\d+:\d+', '', victim_name).strip()
        
        # Очистка от меток [ИИ] / [AI]
        killer_veh_clean = re.sub(r'\[(ии|ai)\]\s*', '', killer_veh or "", 
                                  flags=re.IGNORECASE).strip()
        victim_veh_clean = re.sub(r'\[(ии|ai)\]\s*', '', victim_veh or "", 
                                  flags=re.IGNORECASE).strip()
        
        # Получение информации о технике
        killer_info = self.find_vehicle_info(killer_veh_clean)
        victim_info = self.find_vehicle_info(victim_veh_clean)
        
        # Определение врага
        is_enemy = self._is_enemy(killer_veh_clean, killer_info, victim_info)
        
        # Обновление статистики убийцы
        killer_player = self.get_player(killer_name)
        killer_player.kills += 1
        
        if killer_info and self.is_air_vehicle(killer_info.get('type', 'tank')):
            killer_player.is_disqualified = True
        
        # Добавление в список активных врагов
        if is_enemy:
            self._add_active_enemy(killer_name, killer_veh or killer_veh_clean, killer_info)
        
        # Проверка на возможность ядерки
        self.check_nuke(killer_player, is_enemy)
        
        # Обновление статистики жертвы (игнорируем разведывательные дроны)
        if "recon micro" not in victim_veh_clean.lower():
            self._process_victim(victim_name, victim_veh or victim_veh_clean, victim_info)
        
        return True

    def _process_solo_death(self, text: str) -> bool:
        """Обработать событие самоуничтожения. Возвращает True если событие обработано"""
        solo_found = None
        for act in self.SOLO_ACTIONS:
            if act in text:
                solo_found = act
                break
        
        if not solo_found:
            return False
        
        parts = text.split(solo_found, 1)
        if not parts:
            return False
        
        player_name, vehicle = self.parse_entity(parts[0].strip())
        if not player_name:
            return False
        
        # Убираем временную метку
        player_name = re.sub(r'^\d+:\d+', '', player_name).strip()
        
        # Обновление статистики
        info = self.find_vehicle_info(vehicle)
        victim_player = self.get_player(player_name)
        victim_player.deaths += 1
        
        if info and self.is_air_vehicle(info.get('type', 'tank')):
            victim_player.is_disqualified = True
        
        # Удаление из активных врагов
        self._process_victim(player_name, vehicle, info)
        
        return True

    def _is_enemy(self, veh_clean: str, killer_info: Optional[dict], 
                  victim_info: Optional[dict]) -> bool:
        """Определить, является ли убийца врагом"""
        # Особый случай: MQ-1 дрон
        if veh_clean == "MQ-1":
            if victim_info:
                victim_nation = victim_info.get('nation')
                if victim_nation and victim_nation not in self.selected_nations:
                    return True
        # Обычная проверка по нации
        elif killer_info:
            if killer_info.get('nation') in self.selected_nations:
                return True
        
        return False

    def _add_active_enemy(self, name: str, vehicle: str, info: Optional[dict]):
        """Добавить врага в список активных"""
        # Проверка на повторное появление после смерти (респаун)
        if name in self.dead_enemies:
            last_death = self.dead_enemies[name]
            # Если прошло меньше 25 секунд с момента смерти на той же технике - игнорируем
            if (last_death["veh"] == vehicle and 
                time.time() - last_death["time"] < 25):
                return
            else:
                del self.dead_enemies[name]
        
        # Добавление в активные враги
        self.active_enemies[name] = {
            "veh": vehicle,
            "nation": info.get('nation', 'unknown') if info else 'unknown',
            "type": info.get('type', 'special') if info else 'aircraft',
            "note": info.get('note', '') if info else ''
        }

    def _process_victim(self, name: str, vehicle: str, info: Optional[dict]):
        """Обработать смерть игрока"""
        victim_player = self.get_player(name)
        victim_player.deaths += 1
        
        if info and self.is_air_vehicle(info.get('type', 'tank')):
            victim_player.is_disqualified = True
        
        # Удаление из активных врагов и добавление в мертвых
        death_veh_name = vehicle
        if name in self.active_enemies:
            death_veh_name = self.active_enemies[name]["veh"]
            del self.active_enemies[name]
        
        self.dead_enemies[name] = {
            "veh": death_veh_name,
            "time": time.time()
        }

    def update_data(self):
        """Основной метод обновления данных через API"""
        self.check_battle_status()
        
        if not self.is_api_available or not self.is_in_battle:
            return
        
        try:
            # Запрос событий из API
            req_last_evt = 0 if self.skip_history_mode else self.last_dmg_id
            r = self.session.get(
                f"{API_URL}/hudmsg?lastEvt=-1&lastDmg={req_last_evt}", 
                timeout=0.5
            )
            
            if r.status_code != 200:
                return
            
            data = r.json()
            messages = data.get("damage", []) or []
            
            if not messages:
                if self.skip_history_mode:
                    self.skip_history_mode = False
                return
            
            # Первый запуск - пропускаем историю
            if self.skip_history_mode:
                self.last_dmg_id = messages[-1].get("id", 0)
                self.skip_history_mode = False
                return
            
            # Обработка новых сообщений
            max_id = self.last_dmg_id
            
            for msg in messages:
                msg_id = msg.get("id", 0)
                if msg_id <= self.last_dmg_id:
                    continue
                
                text = msg.get("msg", "") or ""
                
                # Попытка обработать как убийство или самоуничтожение
                if not self._process_kill_event(text):
                    self._process_solo_death(text)
                
                if msg_id > max_id:
                    max_id = msg_id
            
            self.last_dmg_id = max_id
            
        except Exception as e:
            print(f"[ERROR] update_data: {e}")

    def check_nuke(self, player: PlayerStats, is_enemy: bool):
        """Проверить возможность вызова ядерки и проиграть звук"""
        if player.can_nuke and not player.alert_played:
            self.sound_callback('enemy' if is_enemy else 'ally')
            player.alert_played = True


# ==================== ПОТОКИ ====================

class GamePoller(QThread):
    """Поток для периодического опроса API игры"""
    data_updated = pyqtSignal()
    
    def __init__(self, logic: WTLogic):
        super().__init__()
        self.logic = logic
        self.running = False
    
    def run(self):
        self.running = True
        while self.running:
            self.logic.update_data()
            self.data_updated.emit()
            time.sleep(1)
    
    def stop(self):
        self.running = False
        self.wait()


class KeyPollingWorker(QThread):
    """Поток для отслеживания глобальных хоткеев"""
    scan_signal = pyqtSignal()
    toggle_overlay_signal = pyqtSignal()
    
    def __init__(self, scan_key: str, overlay_key: str):
        super().__init__()
        self.scan_key = scan_key
        self.overlay_key = overlay_key
        self.running = True
    
    def update_keys(self, scan: str, overlay: str):
        """Обновить горячие клавиши"""
        self.scan_key = scan
        self.overlay_key = overlay
    
    def run(self):
        if not keyboard:
            return
        
        is_scan_pressed = False
        is_ov_pressed = False
        
        while self.running:
            try:
                # Сканирование флагов
                if self.scan_key and keyboard.is_pressed(self.scan_key):
                    if not is_scan_pressed:
                        self.scan_signal.emit()
                        is_scan_pressed = True
                else:
                    is_scan_pressed = False
                
                # Переключение оверлея
                if self.overlay_key and keyboard.is_pressed(self.overlay_key):
                    if not is_ov_pressed:
                        self.toggle_overlay_signal.emit()
                        is_ov_pressed = True
                else:
                    is_ov_pressed = False
                
                time.sleep(0.05)
            except Exception:
                pass
    
    def stop(self):
        self.running = False
        self.wait()


# ==================== ОВЕРЛЕЙ ====================


class Overlay(QWidget):
    """Оверлей для отображения информации о врагах поверх игры"""
    
    def __init__(self, logic: WTLogic):
        super().__init__()
        self.logic = logic
        
        # Настройка прозрачного окна поверх всех окон
        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint |
            Qt.WindowType.WindowStaysOnTopHint |
            Qt.WindowType.Tool
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setWindowFlag(Qt.WindowType.WindowTransparentForInput, True)
        
        # UI
        self.layout = QVBoxLayout()
        self.label = QLabel("ТЕСТ")
        self.layout.addWidget(self.label)
        self.setLayout(self.layout)
        self.setGeometry(30, 150, 400, 400)
        
        # Параметры отображения
        self.font_size = 20
        self.show_border = False  # Красная рамка для настройки позиции
        self.temp_test_mode = False  # Тестовый режим при запуске
        
        # Сообщение сканера флагов
        self.scan_message = ""
        self.scan_timer = QTimer()
        self.scan_timer.setSingleShot(True)
        self.scan_timer.timeout.connect(self.clear_scan_message)
        
        self.update_style()
    
    def update_style(self):
        """Обновить стили (размер текста, рамка)"""
        border = ("border: 2px dashed rgba(255, 255, 255, 150);" 
                 if self.show_border else "border: none;")
        self.label.setStyleSheet(
            f"font-size: {self.font_size}px; "
            f"font-weight: bold; "
            f"font-family: 'Segoe UI'; "
            f"{border}"
        )
    
    def is_visible_by_filter(self, nation: str, v_type: str) -> bool:
        """Проверить, подходит ли техника под активные фильтры"""
        # Неизвестные нации всегда показываем
        if nation == "unknown":
            return True
        
        # Фильтр по нации
        if nation not in self.logic.selected_nations:
            return False
        
        # Определение ключа типа для фильтра
        v_type_lower = v_type.lower()
        if "tank" in v_type_lower or "mbt" in v_type_lower:
            type_key = "tank"
        elif "plane" in v_type_lower or "fighter" in v_type_lower:
            type_key = "plane"
        elif "heli" in v_type_lower:
            type_key = "heli"
        elif "spaa" in v_type_lower or "zsu" in v_type_lower:
            type_key = "zsu"
        elif "aircraft" in v_type_lower:
            type_key = "plane"
        else:
            type_key = "special"
        
        # Фильтр по типу
        return type_key in self.logic.selected_types
    
    def show_scan_result(self, nations: List[str]):
        """Показать результат сканирования флагов"""
        if not nations:
            self.scan_message = (
                "<span style='color:#FFFF00;'>🔎 СКАН: НИЧЕГО НЕ НАЙДЕНО</span>"
            )
        else:
            rus_names = [NATION_DISPLAY_NAMES.get(n, n) for n in nations]
            names_str = ", ".join(rus_names)
            self.scan_message = (
                f"<span style='color:#FFFF00;'>🔎 СКАН: [{names_str}]</span>"
            )
        
        self.update_view()
        self.scan_timer.start(10000)  # Показать на 10 секунд
    
    def clear_scan_message(self):
        """Очистить сообщение сканера"""
        self.scan_message = ""
        self.update_view()
    
    def update_view(self):
        """Обновить содержимое оверлея"""
        # Режим настройки - показываем только рамку
        if self.show_border:
            self.label.setText("<span style='color:red'>НАСТРОЙКА ОВЕРЛЕЯ</span>")
            return
        
        text_content = ""
        
        # Сообщение сканера флагов
        if self.scan_message:
            text_content += f"{self.scan_message}<br>"
        
        # Тестовый режим при запуске
        if self.temp_test_mode:
            text_content += "<span style='color:#00FF00'>✓ ЗАПУЩЕНО</span>"
        
        # Список врагов в бою
        elif self.logic.is_in_battle:
            enemy_lines = []
            current_enemies = self.logic.active_enemies.copy()
            
            for name, data in current_enemies.items():
                veh_name = data["veh"]
                nation = data["nation"]
                v_type = str(data["type"]).lower()
                
                # Фильтрация
                if not self.is_visible_by_filter(nation, v_type):
                    continue
                
                # Проверка на возможность ядерки
                player_stats = self.logic.all_players.get(name)
                is_nuke = player_stats and player_stats.can_nuke
                
                # Формирование текста
                if self.logic.show_names or is_nuke:
                    display_text = f"{veh_name} ({name})"
                else:
                    display_text = veh_name
                
                # Добавление заметки
                if self.logic.show_notes:
                    note_text = data.get("note", "")
                    if note_text:
                        display_text += f" — {note_text}"
                
                # Иконка типа техники
                icon = VEHICLE_TYPE_ICONS.get(v_type, "• ")
                
                # Цвет и форматирование
                if is_nuke:
                    line = (f"<span style='color:#00FFFF'>☢️ {display_text} "
                           f"[{player_stats.kills} K]</span>")
                else:
                    line = f"<span style='color:#FF0000'>{icon}{display_text}</span>"
                
                enemy_lines.append(line)
            
            if enemy_lines:
                text_content += "⚠️ ВРАГИ:<br>" + "<br>".join(enemy_lines)
        
        self.label.setText(text_content or " ")



# ==================== СКАНЕР ФЛАГОВ ====================

class FlagScannerWorker(QThread):
    """
    Поток для распознавания флагов наций на экране
    Поддерживает два метода:
    1. OpenCV (cv2) - точный с цветовой валидацией
    2. PyAutoGUI - fallback если OpenCV недоступен
    """
    flags_found = pyqtSignal(list)
    
    # Цветовые диапазоны в HSV для валидации флагов
    COLOR_BLUE = (np.array([90, 50, 50]), np.array([130, 255, 255]))  # Франция, США
    COLOR_GREEN = (np.array([35, 50, 50]), np.array([85, 255, 255]))  # Италия
    COLOR_RED_1 = (np.array([0, 50, 50]), np.array([10, 255, 255]))   # Красный (низкие H)
    COLOR_RED_2 = (np.array([170, 50, 50]), np.array([180, 255, 255])) # Красный (высокие H)

    def __init__(self, region: Tuple[int, int, int, int], flags_dir: str, confidence: float = 0.70):
        super().__init__()
        self.region = region  # (x, y, width, height)
        self.flags_dir = flags_dir
        self.CONFIDENCE = confidence  # Порог совпадения для cv2.matchTemplate
        self.SLEEP_BEFORE = 0.25  # Задержка перед сканированием 

    def check_specific_colors(self, crop_hsv, filename: str) -> bool:
        """
        Цветовая валидация флага для защиты от ложных срабатываний
        
        Проверяет обязательные цвета в вырезанном изображении флага:
        - Италия: должен быть зелёный (>15%), не должно быть синего (<5%)
        - Франция: должен быть синий (>15%), не должно быть зелёного (<5%)  
        - США: должен быть и синий (>8%), и красный (>25%)
        
        Args:
            crop_hsv: Вырезанное изображение в HSV формате
            filename: Имя файла флага для определения страны
            
        Returns:
            True если цвета соответствуют, False если нет
        """
        if not CV2_AVAILABLE or cv2 is None or np is None:
            return True
        
        fn = filename.lower()
        total_pixels = crop_hsv.shape[0] * crop_hsv.shape[1]
        
        if "italy" in fn or "италия" in fn:
            # Италия: зелёный обязателен, синий запрещён
            mask_green = cv2.inRange(crop_hsv, *self.COLOR_GREEN)
            green_pct = np.sum(mask_green > 0) / total_pixels
            
            mask_blue = cv2.inRange(crop_hsv, *self.COLOR_BLUE)
            blue_pct = np.sum(mask_blue > 0) / total_pixels
            
            return green_pct > 0.15 and blue_pct < 0.05
        
        elif "france" in fn or "франция" in fn:
            # Франция: синий обязателен, зелёный запрещён
            mask_blue = cv2.inRange(crop_hsv, *self.COLOR_BLUE)
            blue_pct = np.sum(mask_blue > 0) / total_pixels
            
            mask_green = cv2.inRange(crop_hsv, *self.COLOR_GREEN)
            green_pct = np.sum(mask_green > 0) / total_pixels
            
            return blue_pct > 0.15 and green_pct < 0.05
        
        elif "usa" in fn or "сша" in fn:
            # США: обязательны и синий, и красный
            mask_blue = cv2.inRange(crop_hsv, *self.COLOR_BLUE)
            mask_red1 = cv2.inRange(crop_hsv, *self.COLOR_RED_1)
            mask_red2 = cv2.inRange(crop_hsv, *self.COLOR_RED_2)
            mask_red = mask_red1 + mask_red2
            
            blue_pct = np.sum(mask_blue > 0) / total_pixels
            red_pct = np.sum(mask_red > 0) / total_pixels
            
            return blue_pct > 0.08 and red_pct > 0.25
        
        # Для остальных флагов пропускаем валидацию
        return True

    def run(self):
        """Основной метод сканирования флагов"""
        print(f"[SCAN] Запуск сканера. Зона: {self.region}")
        time.sleep(self.SLEEP_BEFORE)
        
        try:
            found_nations = []
            
            # === МЕТОД 1: OpenCV (рекомендуется) ===
            if CV2_AVAILABLE and cv2 and np:
                found_nations = self._scan_with_opencv()
            
            # === МЕТОД 2: Fallback на PyAutoGUI ===
            else:
                found_nations = self._scan_with_pyautogui()
            
            # Убираем дубликаты
            unique = list(dict.fromkeys(found_nations))
            
            # Сохранение debug-скриншота при пустом результате
            if not unique:
                self._save_debug_screenshot()
            
            self.flags_found.emit(unique)
            
        except Exception as e:
            print(f"[ERROR] Scan error: {e}")
            import traceback
            traceback.print_exc()
            self.flags_found.emit([])
    
    def _scan_with_opencv(self) -> List[str]:
        """Сканирование флагов с помощью OpenCV (точный метод)"""
        if not pyautogui:
            print("[ERROR] pyautogui недоступен")
            return []
        
        # Захват скриншота и конвертация в формат OpenCV
        screenshot_pil = pyautogui.screenshot(region=self.region)
        screenshot_cv = cv2.cvtColor(np.array(screenshot_pil), cv2.COLOR_RGB2BGR)
        h_img, w_img, _ = screenshot_cv.shape
        
        # Сохраняем для debug
        self._last_screenshot = screenshot_pil
        
        found = []
        
        if not os.path.exists(self.flags_dir):
            print(f"[ERROR] Папка flags не найдена: {self.flags_dir}")
            return found
        
        # Перебор всех файлов флагов
        for filename in os.listdir(self.flags_dir):
            if not filename.lower().endswith(('.png', '.jpg', '.jpeg')):
                continue
            
            nation_name = os.path.splitext(filename)[0]
            path = os.path.join(self.flags_dir, filename)
            
            # Загрузка шаблона
            try:
                template = cv2.imread(path, cv2.IMREAD_COLOR)
                if template is None:
                    print(f"[WARN] Не удалось загрузить {filename}")
                    continue
            except Exception as e:
                print(f"[ERROR] Ошибка загрузки {filename}: {e}")
                continue
            
            h_temp, w_temp, _ = template.shape
            if h_temp > h_img or w_temp > w_img:
                continue
            
            # Поиск по геометрии (Template Matching)
            result = cv2.matchTemplate(screenshot_cv, template, cv2.TM_CCOEFF_NORMED)
            _, max_val, _, max_loc = cv2.minMaxLoc(result)
            
            if max_val >= self.CONFIDENCE:
                # Вырезаем найденный фрагмент
                top_left = max_loc
                crop_img = screenshot_cv[
                    top_left[1]:top_left[1]+h_temp,
                    top_left[0]:top_left[0]+w_temp
                ]
                crop_hsv = cv2.cvtColor(crop_img, cv2.COLOR_BGR2HSV)
                
                # Цветовая валидация
                if self.check_specific_colors(crop_hsv, filename):
                    print(f"[OK] НАЙДЕН: {nation_name} (геометрия: {max_val:.2f}, цвета: ОК)")
                    found.append(nation_name)
                else:
                    print(f"[REJECTED] {nation_name} (геометрия: {max_val:.2f}, цвета: НЕТ)")
            else:
                print(f"[MISS] {nation_name}: не найден (макс: {max_val:.2f})")
        
        return found
    
    def _scan_with_pyautogui(self) -> List[str]:
        """Fallback сканирование с PyAutoGUI (менее точный)"""
        if not pyautogui:
            print("[ERROR] pyautogui недоступен")
            return []
        
        print("[INFO] OpenCV недоступен, использую PyAutoGUI (точность ниже)")
        
        screenshot = pyautogui.screenshot(region=self.region)
        self._last_screenshot = screenshot
        
        SCALES = [1.00, 0.95, 1.05, 0.90, 1.10]
        CONFIDENCES = [0.90, 0.85, 0.80]
        
        found = []
        
        if not os.path.exists(self.flags_dir):
            print(f"[ERROR] Папка flags не найдена: {self.flags_dir}")
            return found
        
        for filename in os.listdir(self.flags_dir):
            if not filename.lower().endswith(('.png', '.jpg', '.jpeg')):
                continue
            
            nation_name = os.path.splitext(filename)[0]
            path = os.path.join(self.flags_dir, filename)
            
            try:
                tmpl_orig = Image.open(path).convert('RGB')
            except Exception as e:
                print(f"[ERROR] Не удалось открыть {filename}: {e}")
                continue
            
            tw, th = tmpl_orig.size
            matched = False
            
            # Перебор масштабов и порогов
            for scale in SCALES:
                if matched:
                    break
                
                try:
                    if scale == 1.0:
                        tmpl = tmpl_orig
                    else:
                        new_w = max(1, int(tw * scale))
                        new_h = max(1, int(th * scale))
                        tmpl = tmpl_orig.resize((new_w, new_h), Image.LANCZOS)
                except Exception:
                    continue
                
                for conf in CONFIDENCES:
                    try:
                        loc = pyautogui.locate(tmpl, screenshot, confidence=conf)
                        if loc:
                            print(f"[OK] {nation_name} scale={scale:.2f} conf={conf}")
                            found.append(nation_name)
                            matched = True
                            break
                    except TypeError:
                        # Старая версия PyAutoGUI без confidence
                        try:
                            loc = pyautogui.locate(tmpl, screenshot)
                            if loc:
                                print(f"[OK] {nation_name} (no conf)")
                                found.append(nation_name)
                                matched = True
                                break
                        except Exception:
                            break
                    except Exception:
                        break
        
        return found
    
    def _save_debug_screenshot(self):
        """Сохранить debug-скриншот при отсутствии результатов"""
        if not hasattr(self, '_last_screenshot'):
            return
        
        try:
            dbg_path = os.path.join(BASE_DIR, "last_scan_debug.png")
            self._last_screenshot.save(dbg_path)
            print(f"[DEBUG] Скриншот сохранён: {dbg_path}")
        except Exception as e:
            print(f"[ERROR] Не удалось сохранить debug скриншот: {e}")



# ==================== ГЛАВНОЕ ОКНО ПРИЛОЖЕНИЯ ====================

class Settings(QWidget):
    """Главное окно настроек и управления трекером"""
    def __init__(self):
        super().__init__()
        self.player = QMediaPlayer()
        self.audio_output = QAudioOutput()
        self.player.setAudioOutput(self.audio_output)
        self.logic = WTLogic(self.play_sound)
        self.ov = Overlay(self.logic) 
        self.poller = GamePoller(self.logic)
        self.poller.data_updated.connect(self.ov.update_view) 
        self.scan_region = None 
        self.temp_top_left = None
        self.current_scan_hotkey = "/" 
        self.current_overlay_hotkey = "f8"
        self.is_overlay_visible = True
        self.tracking_started = False
        self.hooks = {} 
        self.init_ui()
        self.load_config() 
        self.logic.load_db()
        self.update_db_counter()
        self.key_worker = KeyPollingWorker(self.current_scan_hotkey, self.current_overlay_hotkey)
        self.key_worker.scan_signal.connect(self.start_flag_scan)
        self.key_worker.toggle_overlay_signal.connect(self.toggle_ov_visibility)
        self.key_worker.start()
        self.ui_timer = QTimer()
        self.ui_timer.timeout.connect(self.update_status_display)
        self.update_filters()
        self.update_ov_status_label()
        
        # Автостарт отслеживания (заменяет функционал кнопки)
        self.tracking_started = True
        self.ui_timer.start(1000)      
        self.poller.start()            
        self.ov.show()

    def update_keys_in_worker(self):
        if self.key_worker:
            self.key_worker.update_keys(self.current_scan_hotkey, self.current_overlay_hotkey)
    def enable_setup_hotkeys(self):
        if not keyboard: return
        try:
            if "setup_f6" not in self.hooks:
                self.hooks["setup_f6"] = keyboard.add_hotkey('F6', self.set_top_left)
            if "setup_f7" not in self.hooks:
                self.hooks["setup_f7"] = keyboard.add_hotkey('F7', self.set_bottom_right)
            print("Режим настройки: F6 и F7 активированы.")
        except Exception as e: print("Ошибка setup keys: ", e)
    def disable_setup_hotkeys(self):
        if not keyboard: return
        try:
            if "setup_f6" in self.hooks:
                keyboard.remove_hotkey(self.hooks["setup_f6"])
                del self.hooks["setup_f6"]
            if "setup_f7" in self.hooks:
                keyboard.remove_hotkey(self.hooks["setup_f7"])
                del self.hooks["setup_f7"]
            print("Режим настройки: F6 и F7 отключены.")
        except Exception: pass
    def play_sound(self, target_type):
        if not hasattr(self, 'cb_sound_enabled') or not self.cb_sound_enabled.isChecked():
            return
        sound_file = SOUND_ENEMY if target_type == 'enemy' else SOUND_ALLY
        if os.path.exists(sound_file):
            self.player.setSource(QUrl.fromLocalFile(sound_file))
            self.audio_output.setVolume(1.0); self.player.play()
    def init_ui(self):
        self.setWindowTitle("WT Tracker")
        self.setFixedSize(380, 600)
        main = QVBoxLayout()
        main.setContentsMargins(10, 10, 10, 10); main.setSpacing(5)
        self.btn_settings = QPushButton("⚙ НАСТРОЙКИ ОВЕРЛЕЯ И СКАНЕРА")
        self.btn_settings.setCheckable(True)
        self.btn_settings.clicked.connect(self.toggle_settings_panel)
        main.addWidget(self.btn_settings)
        self.settings_frame = QFrame()
        self.settings_frame.hide()
        sl_layout = QVBoxLayout()
        screen_layout = QHBoxLayout()
        screen_lbl = QLabel("Монитор: ")
        self.combo_screens = QComboBox()
        screens = QApplication.screens()
        for i, screen in enumerate(screens):
            geo = screen.geometry()
            self.combo_screens.addItem(f"Экран {i} ({geo.width()}x{geo.height()})", i)
        self.combo_screens.currentIndexChanged.connect(self.on_screen_changed)
        screen_layout.addWidget(screen_lbl)
        screen_layout.addWidget(self.combo_screens)
        sl_layout.addLayout(screen_layout)
        screen = QApplication.primaryScreen().geometry()
        self.slider_x = self.create_slider(sl_layout, "Позиция X (отн. экрана)", 0, screen.width(), self.update_ov_preview)
        self.slider_y = self.create_slider(sl_layout, "Позиция Y (отн. экрана)", 0, screen.height(), self.update_ov_preview)
        self.slider_size = self.create_slider(sl_layout, "Размер текста", 10, 60, self.update_ov_preview)
        self.slider_opa = self.create_slider(sl_layout, "Прозрачность %", 10, 100, self.update_ov_preview)
        key_layout = QHBoxLayout()
        key_lbl = QLabel("Кнопка скан: ")
        self.entry_hotkey = QLineEdit("/")
        self.entry_hotkey.setPlaceholderText("напр. /")
        btn_apply_key = QPushButton("Сет")
        btn_apply_key.setFixedWidth(50)
        btn_apply_key.clicked.connect(self.update_scan_hotkey_from_ui)
        key_layout.addWidget(key_lbl)
        key_layout.addWidget(self.entry_hotkey)
        key_layout.addWidget(btn_apply_key)
        sl_layout.addLayout(key_layout)
        key_ov_layout = QHBoxLayout()
        key_ov_lbl = QLabel("Кнопка скрыт: ")
        self.entry_ov_hotkey = QLineEdit("f8")
        self.entry_ov_hotkey.setPlaceholderText("напр. f8")
        btn_apply_ov = QPushButton("Сет")
        btn_apply_ov.setFixedWidth(50)
        btn_apply_ov.clicked.connect(self.update_overlay_hotkey_from_ui)
        key_ov_layout.addWidget(key_ov_lbl)
        key_ov_layout.addWidget(self.entry_ov_hotkey)
        key_ov_layout.addWidget(btn_apply_ov)
        sl_layout.addLayout(key_ov_layout)
        scan_info = QLabel("<b>Настройка зоны (F6/F7 активны только при открытых настройках):</b><br>1. Курсор в левый-верх угол -> <b>F6</b><br>2. Курсор в правый-нижний -> <b>F7</b>")
        scan_info.setStyleSheet("font-size: 10px; color: #aaa; margin-top: 5px;")
        scan_info.setWordWrap(True)
        sl_layout.addWidget(scan_info)
        self.lbl_region = QLabel("Зона сканирования: Не задана")
        self.lbl_region.setStyleSheet("color: #FF4444; font-size: 10px;")
        sl_layout.addWidget(self.lbl_region)
        self.settings_frame.setLayout(sl_layout)
        main.addWidget(self.settings_frame)
        n_group = QGroupBox("1. Нации")
        n_grid = QGridLayout()
        self.n_checks = {}
        nations_map = [
            ("USA", "США"), ("Germany", "Германия"), ("USSR", "СССР"), ("Great Britain", "Британия"),
            ("Japan", "Япония"), ("China", "Китай"), ("Italy", "Италия"), ("France", "Франция"),
            ("Sweden", "Швеция"), ("Israel", "Израиль")
        ]
        row, col = 0, 0
        for db_name, display_name in nations_map:
            cb = QCheckBox(display_name)
            cb.stateChanged.connect(self.update_filters)
            self.n_checks[db_name] = cb
            n_grid.addWidget(cb, row, col)
            col += 1
            if col > 1: col = 0; row += 1
        n_group.setLayout(n_grid)
        main.addWidget(n_group)
        t_group = QGroupBox("2. Тип техники")
        t_layout = QHBoxLayout()
        self.t_checks = {
            "tank": QCheckBox("Танк"), "plane": QCheckBox("Лёт"), "heli": QCheckBox("Вер"),
            "zsu": QCheckBox("ЗСУ"), "special": QCheckBox("Особые")
        }
        for k, v in self.t_checks.items(): 
            v.setChecked(True)
            v.stateChanged.connect(self.update_filters)
            t_layout.addWidget(v)
        t_group.setLayout(t_layout)
        main.addWidget(t_group)
        self.cb_show_notes = QCheckBox("Показывать заметки")
        self.cb_show_notes.setChecked(True) 
        self.cb_show_notes.stateChanged.connect(self.update_filters)
        main.addWidget(self.cb_show_notes)
        self.cb_show_names = QCheckBox("Показывать ники игроков")
        self.cb_show_names.setChecked(True)
        self.cb_show_names.stateChanged.connect(self.update_filters)
        main.addWidget(self.cb_show_names)
        self.cb_sound_enabled = QCheckBox("Включить оповещение")
        self.cb_sound_enabled.setChecked(True) 
        main.addWidget(self.cb_sound_enabled)
        self.lbl_db_count = QLabel("В базе: 0 ед. техники")
        self.lbl_db_count.setStyleSheet("font-size: 11px; color: #777; margin-bottom: 2px;")
        self.lbl_db_count.setAlignment(Qt.AlignmentFlag.AlignRight)
        main.addWidget(self.lbl_db_count)
        
        # Кнопка СТАРТ удалена
        
        self.lbl_ov_status = QLabel("Оверлей: ВИДЕН")
        self.lbl_ov_status.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.lbl_ov_status.setStyleSheet("font-weight: bold; color: #00FF00; margin-bottom: 5px;")
        main.addWidget(self.lbl_ov_status)
        self.status = QLabel("Нажми Старт")
        self.status.setAlignment(Qt.AlignmentFlag.AlignCenter); self.status.setStyleSheet("color: #888;")
        main.addWidget(self.status)
        self.setLayout(main)
    def update_db_counter(self):
        count = len(self.logic.vehicle_db)
        self.lbl_db_count.setText(f"В базе: {count} ед. техники")
    def update_filters(self):
        self.logic.selected_nations = {n for n, cb in self.n_checks.items() if cb.isChecked()}
        self.logic.selected_types = {t for t, cb in self.t_checks.items() if cb.isChecked()}
        self.logic.show_notes = self.cb_show_notes.isChecked()
        self.logic.show_names = self.cb_show_names.isChecked()
        self.ov.update_view()
    def create_slider(self, layout, name, mn, mx, func):
        l = QLabel(name); l.setStyleSheet("font-size: 11px; margin-top: 2px;")
        layout.addWidget(l)
        s = QSlider(Qt.Orientation.Horizontal); s.setRange(mn, mx); s.valueChanged.connect(func)
        layout.addWidget(s); return s
    def update_scan_hotkey_from_ui(self):
        new_key = self.entry_hotkey.text().lower().strip()
        if not new_key: return
        self.current_scan_hotkey = new_key
        self.update_keys_in_worker()
        self.save_config() 
        QMessageBox.information(self, "Успех", f"Кнопка скан: {new_key}")
    def update_overlay_hotkey_from_ui(self):
        new_key = self.entry_ov_hotkey.text().lower().strip()
        if not new_key: return
        self.current_overlay_hotkey = new_key
        self.update_keys_in_worker() 
        self.save_config()
        self.update_ov_status_label() 
        QMessageBox.information(self, "Успех", f"Кнопка скрытия: {new_key}")
    def on_screen_changed(self, index):
        screens = QApplication.screens()
        if 0 <= index < len(screens):
            screen_geo = screens[index].geometry()
            self.slider_x.setRange(0, screen_geo.width())
            self.slider_y.setRange(0, screen_geo.height())
            self.update_ov_preview()
    def set_top_left(self):
        if not self.settings_frame.isVisible(): return
        if pyautogui:
            x, y = pyautogui.position()
            self.temp_top_left = (x, y)
            self.lbl_region.setText(f"Точка 1 задана: {x}, {y}. Жми F7 во второй точке.")
            self.lbl_region.setStyleSheet("color: orange; font-size: 10px;")
    def set_bottom_right(self):
        if not self.settings_frame.isVisible() or not self.temp_top_left: return
        if pyautogui:
            x2, y2 = pyautogui.position()
            x1, y1 = self.temp_top_left
            w = abs(x2 - x1)
            h = abs(y2 - y1)
            final_x = min(x1, x2)
            final_y = min(y1, y2)
            if w < 5 or h < 5: 
                print("Слишком маленькая зона!")
                return
            self.scan_region = (final_x, final_y, w, h)
            self.lbl_region.setText(f"Зона: X={final_x}, Y={final_y}, W={w}, H={h}")
            self.lbl_region.setStyleSheet("color: #00FF00; font-size: 10px;")
            self.save_config() 
    def start_flag_scan(self):
        print(f"Нажат {self.current_scan_hotkey}. Скан...")
        if self.scan_region:
            confidence = getattr(self, 'confidence_threshold', 0.70)
            self.worker = FlagScannerWorker(self.scan_region, FLAGS_DIR, confidence)
            self.worker.flags_found.connect(self.apply_detected_flags)
            self.worker.start()
        else:
            self.ov.show_scan_result([])
    def apply_detected_flags(self, nations):
        self.ov.show_scan_result(nations)
        if not nations: return
        for cb in self.n_checks.values(): cb.blockSignals(True)
        for cb in self.n_checks.values(): cb.setChecked(False)
        count = 0
        for nat_name in nations:
            for key, cb in self.n_checks.items():
                if key.lower() == nat_name.lower():
                    cb.setChecked(True)
                    count += 1
        for cb in self.n_checks.values(): cb.blockSignals(False)
        self.update_filters()
    def toggle_settings_panel(self):
        if self.btn_settings.isChecked():
            self.settings_frame.show();
            self.setFixedSize(380, 880)
            self.ov.show_border = True; self.ov.show();
            self.ov.update_style()
            self.enable_setup_hotkeys()
        else:
            self.settings_frame.hide();
            self.setFixedSize(380, 600)
            self.ov.show_border = False;
            self.ov.update_style()
            if not self.logic.active_enemies: self.ov.label.setText(" ")
            self.disable_setup_hotkeys()
    def update_ov_preview(self):
        screens = QApplication.screens()
        idx = self.combo_screens.currentIndex()
        if 0 <= idx < len(screens):
            target_screen = screens[idx]
            geo = target_screen.geometry()
            global_x = geo.x() + self.slider_x.value()
            global_y = geo.y() + self.slider_y.value()
            self.ov.move(global_x, global_y)
        else:
            self.ov.move(self.slider_x.value(), self.slider_y.value())
        self.ov.font_size = self.slider_size.value()
        self.ov.setWindowOpacity(self.slider_opa.value() / 100.0)
        self.ov.update_style()
    def start_tracking(self):
        self.tracking_started = True 
        if self.btn_settings.isChecked(): 
            self.btn_settings.setChecked(False)
            self.toggle_settings_panel()
        self.update_filters()
        self.ov.temp_test_mode = True;
        self.ov.update_view()
        QTimer.singleShot(3000, self.disable_temp_test)
        if self.is_overlay_visible:
            self.ov.show()
        if not self.poller.isRunning():
            self.poller.start()
        self.ui_timer.start(1000)
        self.status.setText("Поиск игры...")
    def update_status_display(self):
        if not self.tracking_started:
            self.status.setText("Ожидание запуска...")
            return
        if not self.logic.is_api_available:
            self.status.setText("⛔ ИГРА НЕ ЗАПУЩЕНА")
            self.status.setStyleSheet("color: #FF4444; font-weight: bold;")
        elif not self.logic.is_in_battle:
            self.status.setText("⚓ В АНГАРЕ")
            self.status.setStyleSheet("color: #FFA500; font-weight: bold;")
        else:
            self.status.setText("⚔️ В БОЮ (ОТСЛЕЖИВАНИЕ)")
            self.status.setStyleSheet("color: #00FF00; font-weight: bold;")
    def disable_temp_test(self):
        self.ov.temp_test_mode = False
        self.ov.update_view()
    def toggle_ov_visibility(self):
        if self.ov.isVisible(): 
            self.ov.hide()
            self.is_overlay_visible = False
        else: 
            self.ov.show()
            self.is_overlay_visible = True
        self.update_ov_status_label()
    def update_ov_status_label(self):
        if self.is_overlay_visible:
            self.lbl_ov_status.setText(f"Оверлей: ВИДЕН [{self.current_overlay_hotkey}]")
            self.lbl_ov_status.setStyleSheet("font-weight: bold; color: #00FF00; margin-bottom: 5px;")
        else:
            self.lbl_ov_status.setText(f"Оверлей: СКРЫТ [{self.current_overlay_hotkey}]")
            self.lbl_ov_status.setStyleSheet("font-weight: bold; color: #FF4444; margin-bottom: 5px;")
    def save_config(self):
        cfg = {
            "overlay": {
                "x": self.slider_x.value(),
                "y": self.slider_y.value(),
                "size": self.slider_size.value(),
                "opacity": self.slider_opa.value(),
                "monitor_idx": self.combo_screens.currentIndex()
            },
            "filters": {
                "nations": [n for n, cb in self.n_checks.items() if cb.isChecked()],
                "types": [t for t, cb in self.t_checks.items() if cb.isChecked()]
            },
            "scanner": {
                "region": self.scan_region,
                "confidence_threshold": getattr(self, 'confidence_threshold', 0.70),
                "hotkey": self.current_scan_hotkey
            },
            "hotkeys": {
                "scan": self.current_scan_hotkey,
                "overlay_toggle": self.current_overlay_hotkey
            },
            "display": {
                "show_notes": self.cb_show_notes.isChecked(),
                "show_names": self.cb_show_names.isChecked()
            },
            "audio": {
                "enabled": self.cb_sound_enabled.isChecked()
            }
        }
        try:
            with open(CONFIG_FILE, "w", encoding="utf-8") as f:
                json.dump(cfg, f, indent=2, ensure_ascii=False)
            print("Конфиг сохранен.")
        except Exception as e:
            print("save_config error: ", e)
        self.logic.save_unknowns()

    def load_config(self):
        if os.path.exists(CONFIG_FILE):
            try:
                with open(CONFIG_FILE, "r", encoding="utf-8") as f:
                    c = json.load(f)
                
                # Загрузка настроек оверлея
                overlay = c.get("overlay", {})
                mon_idx = overlay.get("monitor_idx", c.get("monitor_idx", 0))  # fallback для старого формата
                if mon_idx < self.combo_screens.count():
                    self.combo_screens.setCurrentIndex(mon_idx)
                self.slider_x.setValue(overlay.get("x", c.get("x", 30)))
                self.slider_y.setValue(overlay.get("y", c.get("y", 150)))
                self.slider_size.setValue(overlay.get("size", c.get("size", 20)))
                self.slider_opa.setValue(overlay.get("opacity", c.get("opa", 100)))
                
                # Загрузка фильтров
                filters = c.get("filters", {})
                for n in filters.get("nations", c.get("nations", [])):
                    if n in self.n_checks:
                        self.n_checks[n].setChecked(True)
                for t in filters.get("types", c.get("types", [])):
                    if t in self.t_checks:
                        self.t_checks[t].setChecked(True)
                
                # Загрузка настроек сканера
                scanner = c.get("scanner", {})
                self.scan_region = scanner.get("region", c.get("scan_region", None))
                self.confidence_threshold = scanner.get("confidence_threshold", 0.70)
                
                # Загрузка горячих клавиш
                hotkeys = c.get("hotkeys", {})
                self.current_scan_hotkey = hotkeys.get("scan", c.get("scan_hotkey", "/"))
                self.entry_hotkey.setText(self.current_scan_hotkey)
                self.current_overlay_hotkey = hotkeys.get("overlay_toggle", c.get("overlay_hotkey", "f8"))
                self.entry_ov_hotkey.setText(self.current_overlay_hotkey)
                
                # Загрузка настроек отображения
                display = c.get("display", {})
                self.cb_show_notes.setChecked(display.get("show_notes", c.get("show_notes", True)))
                self.cb_show_names.setChecked(display.get("show_names", c.get("show_names", True)))
                
                # Загрузка настроек аудио
                audio = c.get("audio", {})
                self.cb_sound_enabled.setChecked(audio.get("enabled", c.get("sound_enabled", True)))
                
                if self.scan_region:
                    self.lbl_region.setText(f"Зона из памяти: {self.scan_region}")
                    self.lbl_region.setStyleSheet("color: #00FF00; font-size: 10px;")
            except Exception as e:
                print("load_config error: ", e)

    def closeEvent(self, event):
        if self.poller.isRunning(): self.poller.stop()
        if self.key_worker.isRunning(): self.key_worker.stop()
        self.save_config()
        event.accept()

if __name__ == "__main__":
    app = QApplication(sys.argv)
    ex = Settings()
    ex.show()
    sys.exit(app.exec())